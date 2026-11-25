use calamine::{open_workbook_auto, Data, Range, Reader, Sheets};
use parking_lot::Mutex;
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::OnceLock;

enum Grid {
    Cells { range: Range<Data>, origin: (usize, usize) },
    Text(Vec<Vec<String>>),
}

pub struct SheetDoc {
    names: Vec<String>,
    frozen: Vec<(u32, u32)>,
    grids: Vec<OnceLock<Result<Grid, String>>>,
    workbook: Mutex<Option<Sheets<BufReader<File>>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetInfo {
    pub rows: usize,
    pub cols: usize,
    pub frozen_rows: u32,
    pub frozen_cols: u32,
}

impl SheetDoc {
    pub fn open_workbook(path: &Path) -> Result<Self, String> {
        let workbook = open_workbook_auto(path).map_err(|e| format!("This workbook couldn't be read ({e})."))?;
        let names = workbook.sheet_names();
        let frozen = xlsx_frozen_panes(path, &names).unwrap_or_else(|| vec![(0, 0); names.len()]);
        Ok(SheetDoc { grids: names.iter().map(|_| OnceLock::new()).collect(), names, frozen, workbook: Mutex::new(Some(workbook)) })
    }

    pub fn open_delimited(path: &Path, tab: bool) -> Result<Self, String> {
        let mut head = [0u8; 4096];
        let n = File::open(path).and_then(|mut f| f.read(&mut head)).map_err(|e| e.to_string())?;
        let first_line = head[..n].split(|&b| b == b'\n').next().unwrap_or_default();
        let count = |d: u8| first_line.iter().filter(|&&b| b == d).count();
        let delimiter = if tab { b'\t' } else { [b',', b';', b'\t', b'|'].into_iter().max_by_key(|&d| count(d)).unwrap_or(b',') };
        let mut reader = csv::ReaderBuilder::new().delimiter(delimiter).has_headers(false).flexible(true).from_path(path).map_err(|e| e.to_string())?;
        let rows: Vec<Vec<String>> = reader
            .byte_records()
            .map_while(Result::ok)
            .map(|r| r.iter().map(|f| String::from_utf8_lossy(f).into_owned()).collect())
            .collect();
        let grids = vec![OnceLock::from(Ok(Grid::Text(rows)))];
        Ok(SheetDoc { names: vec![String::new()], frozen: vec![(0, 0)], grids, workbook: Mutex::new(None) })
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    fn grid(&self, sheet: usize) -> Result<&Grid, String> {
        let cell = self.grids.get(sheet).ok_or("That sheet doesn't exist.")?;
        cell.get_or_init(|| {
            let mut wb = self.workbook.lock();
            let wb = wb.as_mut().ok_or("The workbook is closed.")?;
            let range = wb.worksheet_range(&self.names[sheet]).map_err(|e| format!("This sheet couldn't be read ({e})."))?;
            let origin = range.start().map(|(r, c)| (r as usize, c as usize)).unwrap_or((0, 0));
            Ok(Grid::Cells { range, origin })
        })
        .as_ref()
        .map_err(Clone::clone)
    }

    pub fn info(&self, sheet: usize) -> Result<SheetInfo, String> {
        let (rows, cols) = match self.grid(sheet)? {
            Grid::Cells { range, origin } if !range.is_empty() => {
                let (h, w) = range.get_size();
                (origin.0 + h, origin.1 + w)
            }
            Grid::Cells { .. } => (0, 0),
            Grid::Text(rows) => (rows.len(), rows.iter().map(Vec::len).max().unwrap_or(0)),
        };
        let (frozen_rows, frozen_cols) = self.frozen.get(sheet).copied().unwrap_or((0, 0));
        Ok(SheetInfo { rows, cols, frozen_rows, frozen_cols })
    }

    pub fn cells(&self, sheet: usize, rows: std::ops::Range<usize>, cols: std::ops::Range<usize>) -> Result<Vec<Vec<String>>, String> {
        let grid = self.grid(sheet)?;
        Ok(rows
            .map(|r| {
                cols.clone()
                    .map(|c| match grid {
                        Grid::Cells { range, origin } => {
                            if r < origin.0 || c < origin.1 {
                                return String::new();
                            }
                            range.get((r - origin.0, c - origin.1)).map(format_cell).unwrap_or_default()
                        }
                        Grid::Text(rows) => rows.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default(),
                    })
                    .collect()
            })
            .collect())
    }
}

fn format_cell(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{f:.0}"),
        Data::Float(f) => {
            let s = format!("{f:.10}");
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        }
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Data::DateTime(dt) if dt.is_duration() => format_duration(dt.as_f64()),
        Data::DateTime(dt) => format_serial_date(dt.as_f64()),
        Data::Error(e) => e.to_string(),
    }
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

fn format_serial_date(serial: f64) -> String {
    let days = serial.floor() as i64;
    let seconds = ((serial - serial.floor()) * 86_400.0).round() as i64;
    let (y, m, d) = civil_from_days(days - 25_569);
    let time = format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60);
    match (days, seconds) {
        (0, _) => time,
        (_, 0) => format!("{y:04}-{m:02}-{d:02}"),
        _ => format!("{y:04}-{m:02}-{d:02} {time}"),
    }
}

fn format_duration(days: f64) -> String {
    let seconds = (days * 86_400.0).round() as i64;
    format!("{}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)
}

fn xlsx_frozen_panes(path: &Path, names: &[String]) -> Option<Vec<(u32, u32)>> {
    let mut zip = zip::ZipArchive::new(File::open(path).ok()?).ok()?;
    let mut read = |name: &str, limit: u64| -> Option<String> {
        let mut s = String::new();
        zip.by_name(name).ok()?.take(limit).read_to_string(&mut s).ok()?;
        Some(s)
    };
    let workbook = read("xl/workbook.xml", 1 << 20)?;
    let rels = read("xl/_rels/workbook.xml.rels", 1 << 20)?;
    let attr = |tag: &str, key: &str| -> Option<String> {
        let padded = format!(" {tag}");
        let start = padded.find(&format!(" {key}=\""))? + key.len() + 3;
        Some(padded[start..].split('"').next()?.to_owned())
    };
    let sheet_tags: Vec<&str> = workbook.split("<sheet ").skip(1).map(|t| t.split('>').next().unwrap_or("")).collect();
    let rel_tags: Vec<&str> = rels.split("<Relationship ").skip(1).map(|t| t.split('>').next().unwrap_or("")).collect();
    Some(
        names
            .iter()
            .map(|name| {
                let mut pane = || -> Option<(u32, u32)> {
                    let tag = sheet_tags.iter().find(|t| attr(t, "name").as_deref() == Some(name.as_str()))?;
                    let id = attr(tag, "r:id")?;
                    let rel = rel_tags.iter().find(|t| attr(t, "Id").as_deref() == Some(id.as_str()))?;
                    let target = attr(rel, "Target")?;
                    let target = target.strip_prefix('/').map(str::to_owned).unwrap_or_else(|| format!("xl/{target}"));
                    let head = read(&target, 64 << 10)?;
                    let pane = head.split("<pane ").nth(1)?.split('>').next()?;
                    if !pane.contains("state=\"frozen") {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_dates_match_excel() {
        assert_eq!(format_serial_date(45_658.0), "2025-01-01");
        assert_eq!(format_serial_date(45_658.5), "2025-01-01 12:00:00");
        assert_eq!(format_serial_date(0.25), "06:00:00");
        assert_eq!(format_cell(&Data::Float(3.0)), "3");
        assert_eq!(format_cell(&Data::Float(0.1 + 0.2)), "0.3");
    }

    #[test]
    fn frozen_panes_come_from_the_sheet_view() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("lumen-pane-{}.xlsx", std::process::id()));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        for (name, data) in [
            ("xl/workbook.xml", r#"<workbook><sheets><sheet name="A" sheetId="1" r:id="rId1"/><sheet name="B" sheetId="2" r:id="rId2"/></sheets></workbook>"#),
            ("xl/_rels/workbook.xml.rels", r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Target="/xl/worksheets/sheet2.xml"/></Relationships>"#),
            ("xl/worksheets/sheet1.xml", r#"<worksheet><sheetViews><sheetView><pane xSplit="2" ySplit="1" state="frozen"/></sheetView></sheetViews></worksheet>"#),
            ("xl/worksheets/sheet2.xml", r#"<worksheet><sheetViews><sheetView><pane xSplit="3" state="split"/></sheetView></sheetViews></worksheet>"#),
        ] {
            w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(data.as_bytes()).unwrap();
        }
        w.finish().unwrap();
        assert_eq!(xlsx_frozen_panes(&path, &["A".into(), "B".into()]).unwrap(), vec![(1, 2), (0, 0)]);
    }

    #[test]
    fn csv_detects_semicolons_and_keeps_quoted_newlines() {
        let path = std::env::temp_dir().join(format!("lumen-csv-{}.csv", std::process::id()));
        std::fs::write(&path, "name;note\nAna;\"two\nlines\"\nBo;x;extra\n").unwrap();
        let doc = SheetDoc::open_delimited(&path, false).unwrap();
        let info = doc.info(0).unwrap();
        assert_eq!((info.rows, info.cols), (3, 3));
        assert_eq!(doc.cells(0, 1..2, 0..2).unwrap(), vec![vec!["Ana".to_string(), "two\nlines".into()]]);
    }
}
