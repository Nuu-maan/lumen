//! Parts of an xlsx file that calamine doesn't expose: frozen panes and "cell is" conditional formats.

use std::fs::File;
use std::io::Read;
use std::path::Path;

type Zip = zip::ZipArchive<File>;

fn read(zip: &mut Zip, name: &str, limit: u64) -> Option<String> {
    let mut s = String::new();
    zip.by_name(name).ok()?.take(limit).read_to_string(&mut s).ok()?;
    Some(s)
}

/// The value of `key="..."` inside an XML start tag.
fn attr(tag: &str, key: &str) -> Option<String> {
    let padded = format!(" {tag}");
    let needle = format!(" {key}={}", '"');
    let start = padded.find(&needle)? + needle.len();
    Some(padded[start..].split('"').next()?.to_owned())
}

/// The zip path of each named sheet's XML, found through workbook.xml and its relationships.
fn sheet_targets(zip: &mut Zip, names: &[String]) -> Option<Vec<Option<String>>> {
    let workbook = read(zip, "xl/workbook.xml", 1 << 20)?;
    let rels = read(zip, "xl/_rels/workbook.xml.rels", 1 << 20)?;
    let sheet_tags: Vec<&str> = workbook.split("<sheet ").skip(1).map(|t| t.split('>').next().unwrap_or("")).collect();
    let rel_tags: Vec<&str> = rels.split("<Relationship ").skip(1).map(|t| t.split('>').next().unwrap_or("")).collect();
    Some(
        names
            .iter()
            .map(|name| {
                let tag = sheet_tags.iter().find(|t| attr(t, "name").as_deref() == Some(name.as_str()))?;
                let id = attr(tag, "r:id")?;
                let rel = rel_tags.iter().find(|t| attr(t, "Id").as_deref() == Some(id.as_str()))?;
                let target = attr(rel, "Target")?;
                Some(target.strip_prefix('/').map(str::to_owned).unwrap_or_else(|| format!("xl/{target}")))
            })
            .collect(),
    )
}

/// Frozen rows and columns for each sheet, read from the sheet view at the start of its XML.
pub fn frozen_panes(path: &Path, names: &[String]) -> Option<Vec<(u32, u32)>> {
    let mut zip = zip::ZipArchive::new(File::open(path).ok()?).ok()?;
    let targets = sheet_targets(&mut zip, names)?;
    Some(
        targets
            .iter()
            .map(|target| {
                let mut pane = || -> Option<(u32, u32)> {
                    let head = read(&mut zip, target.as_deref()?, 64 << 10)?;
                    let pane = head.split("<pane ").nth(1)?.split('>').next()?;
                    if !pane.contains(r#"state="frozen"#) {
                        return None;
                    }
                    let num = |k| attr(pane, k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) as u32;
                    Some((num("ySplit"), num("xSplit")))
                };
                pane().unwrap_or((0, 0))
            })
            .collect(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    Between,
    NotBetween,
}

/// A "cell is" conditional format: cells in `ranges` whose value passes the comparison get `color` and `fill`.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    ranges: Vec<[usize; 4]>,
    op: Op,
    a: f64,
    b: f64,
    pub color: Option<String>,
    pub fill: Option<String>,
}

impl Rule {
    pub fn applies(&self, r: usize, c: usize, v: f64) -> bool {
        let (lo, hi) = (self.a.min(self.b), self.a.max(self.b));
        self.ranges.iter().any(|g| (g[0]..=g[2]).contains(&r) && (g[1]..=g[3]).contains(&c))
            && match self.op {
                Op::Lt => v < self.a,
                Op::Le => v <= self.a,
                Op::Gt => v > self.a,
                Op::Ge => v >= self.a,
                Op::Eq => v == self.a,
                Op::Ne => v != self.a,
                Op::Between => (lo..=hi).contains(&v),
                Op::NotBetween => !(lo..=hi).contains(&v),
            }
    }
}

/// "C3" as zero-based (row, column).
fn cell(reference: &str) -> Option<(usize, usize)> {
    let reference = reference.replace('$', "");
    let split = reference.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = reference.split_at(split);
    let col = letters.bytes().try_fold(0usize, |acc, b| b.is_ascii_uppercase().then(|| acc * 26 + (b - b'A' + 1) as usize))?;
    let row: usize = digits.parse().ok()?;
    (col > 0 && row > 0).then(|| (row - 1, col - 1))
}

fn range(reference: &str) -> Option<[usize; 4]> {
    let (a, b) = reference.split_once(':').unwrap_or((reference, reference));
    let ((r0, c0), (r1, c1)) = (cell(a)?, cell(b)?);
    Some([r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1)])
}

/// "FF9C0006" (ARGB) as "#9c0006".
fn argb(tag: &str) -> Option<String> {
    let rgb = attr(tag, "rgb")?;
    (rgb.len() == 8).then(|| format!("#{}", rgb[2..].to_ascii_lowercase()))
}

/// The font colour and fill of each differential format in styles.xml, by dxf id.
fn dxfs(styles: &str) -> Vec<(Option<String>, Option<String>)> {
    let Some(block) = styles.split("<dxfs").nth(1).and_then(|s| s.split("</dxfs>").next()) else {
        return Vec::new();
    };
    block
        .split("<dxf>")
        .skip(1)
        .map(|dxf| {
            let font = dxf.split("<font").nth(1).and_then(|f| f.split("</font>").next()).unwrap_or("");
            let color = font.split("<color ").nth(1).and_then(argb);
            let fill = dxf.split("<bgColor ").nth(1).or_else(|| dxf.split("<fgColor ").nth(1)).and_then(argb);
            (color, fill)
        })
        .collect()
}

fn rules(tail: &str, formats: &[(Option<String>, Option<String>)]) -> Vec<Rule> {
    let mut out = Vec::new();
    for block in tail.split("<conditionalFormatting ").skip(1) {
        let head = block.split('>').next().unwrap_or("");
        let ranges: Vec<[usize; 4]> = attr(head, "sqref").unwrap_or_default().split_whitespace().filter_map(range).collect();
        let block = block.split("</conditionalFormatting>").next().unwrap_or("");
        for rule in block.split("<cfRule ").skip(1) {
            let tag = rule.split('>').next().unwrap_or("");
            if attr(tag, "type").as_deref() != Some("cellIs") {
                continue;
            }
            let op = match attr(tag, "operator").as_deref() {
                Some("lessThan") => Op::Lt,
                Some("lessThanOrEqual") => Op::Le,
                Some("greaterThan") => Op::Gt,
                Some("greaterThanOrEqual") => Op::Ge,
                Some("equal") => Op::Eq,
                Some("notEqual") => Op::Ne,
                Some("between") => Op::Between,
                Some("notBetween") => Op::NotBetween,
                _ => continue,
            };
            let values: Vec<f64> = rule
                .split("<formula>")
                .skip(1)
                .filter_map(|f| f.split("</formula>").next()?.trim().parse().ok())
                .collect();
            let Some(&a) = values.first() else { continue };
            let Some((color, fill)) = attr(tag, "dxfId").and_then(|id| id.parse::<usize>().ok()).and_then(|id| formats.get(id)).cloned() else {
                continue;
            };
            out.push(Rule { ranges: ranges.clone(), op, a, b: values.get(1).copied().unwrap_or(a), color, fill });
        }
    }
    out
}

/// The "cell is" conditional formats of one sheet. Only the XML after the cell data is kept.
pub fn conditional_rules(path: &Path, name: &str) -> Vec<Rule> {
    let load = || -> Option<Vec<Rule>> {
        let mut zip = zip::ZipArchive::new(File::open(path).ok()?).ok()?;
        let target = sheet_targets(&mut zip, &[name.to_owned()])?.pop()??;
        let styles = read(&mut zip, "xl/styles.xml", 16 << 20).unwrap_or_default();
        let mut bytes = Vec::new();
        zip.by_name(&target).ok()?.read_to_end(&mut bytes).ok()?;
        let end = bytes.windows(12).rposition(|w| w == b"</sheetData>").unwrap_or(0);
        let tail = String::from_utf8_lossy(&bytes[end..]);
        if !tail.contains("<conditionalFormatting") {
            return None;
        }
        Some(rules(&tail, &dxfs(&styles)))
    };
    load().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_parse() {
        assert_eq!(cell("C3"), Some((2, 2)));
        assert_eq!(cell("$AB$10"), Some((9, 27)));
        assert_eq!(range("A1:D1"), Some([0, 0, 0, 3]));
        assert_eq!(range("B2"), Some([1, 1, 1, 1]));
    }

    #[test]
    fn cell_is_rules_parse_and_apply() {
        let styles = r#"<styleSheet><dxfs count="1"><dxf><font><color rgb="FF9C0006"/></font><fill><patternFill><bgColor rgb="FFFFC7CE"/></patternFill></fill></dxf></dxfs></styleSheet>"#;
        let sheet = r#"</sheetData><conditionalFormatting sqref="C3:C6 E1"><cfRule type="cellIs" dxfId="0" priority="1" operator="lessThan"><formula>0</formula></cfRule><cfRule type="expression" dxfId="0"><formula>A1>2</formula></cfRule></conditionalFormatting>"#;
        let found = rules(sheet, &dxfs(styles));
        assert_eq!(found.len(), 1);
        let rule = &found[0];
        assert_eq!(rule.color.as_deref(), Some("#9c0006"));
        assert_eq!(rule.fill.as_deref(), Some("#ffc7ce"));
        assert!(rule.applies(3, 2, -0.031));
        assert!(!rule.applies(3, 2, 0.5));
        assert!(!rule.applies(3, 3, -1.0));
        assert!(rule.applies(0, 4, -1.0));
    }
}
