use super::{numfmt, xlsx_parts};
use calamine::{open_workbook_auto, Data, Range, Reader, Sheets, Style, StyleRange};
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

/// How one cell looks, sent only for cells that differ from plain black text on white.
#[derive(Serialize)]
pub struct CellStyle {
    pub r: usize,
    pub c: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
}

/// Number formats, colours, merges and conditional formats of one sheet. They take a second pass over
/// the sheet, so they load after the values and the grid refreshes when they arrive.
#[derive(Default)]
struct Looks {
    styles: Option<StyleRange>,
    merges: Vec<[usize; 4]>,
    rules: Vec<xlsx_parts::Rule>,
}

#[derive(Serialize)]
pub struct SheetStyles {
    /// Merged ranges as [first row, first column, last row, last column].
    pub merges: Vec<[usize; 4]>,
    /// Whether the sheet has any formatting at all, so the grid knows whether to refresh.
    pub styled: bool,
}

#[derive(Serialize)]
pub struct Cells {
    pub values: Vec<Vec<String>>,
    pub styles: Vec<CellStyle>,
}

pub struct SheetDoc {
    path: Option<std::path::PathBuf>,
    names: Vec<String>,
    frozen: Vec<(u32, u32)>,
    grids: Vec<OnceLock<Result<Grid, String>>>,
    looks: Vec<OnceLock<Looks>>,
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
        let frozen = xlsx_parts::frozen_panes(path, &names).unwrap_or_else(|| vec![(0, 0); names.len()]);
        Ok(SheetDoc {
            path: Some(path.to_owned()),
            grids: names.iter().map(|_| OnceLock::new()).collect(),
            looks: names.iter().map(|_| OnceLock::new()).collect(),
            names,
            frozen,
            workbook: Mutex::new(Some(workbook)),
        })
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
        let looks = vec![OnceLock::from(Looks::default())];
        Ok(SheetDoc { path: None, names: vec![String::new()], frozen: vec![(0, 0)], grids, looks, workbook: Mutex::new(None) })
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
        let grid = self.grid(sheet)?;
        let (rows, cols) = match grid {
            Grid::Cells { range, origin, .. } if !range.is_empty() => {
                let (h, w) = range.get_size();
                (origin.0 + h, origin.1 + w)
            }
            Grid::Cells { .. } => (0, 0),
            Grid::Text(rows) => (rows.len(), rows.iter().map(Vec::len).max().unwrap_or(0)),
        };
        let (frozen_rows, frozen_cols) = self.frozen.get(sheet).copied().unwrap_or((0, 0));
        Ok(SheetInfo { rows, cols, frozen_rows, frozen_cols })
    }

    /// Loads the sheet's formatting (xlsx only) if it isn't loaded yet. Slow on big sheets, so the
    /// grid asks for it after showing the values.
    pub fn styles(&self, sheet: usize) -> Result<SheetStyles, String> {
        let looks = self.looks.get(sheet).ok_or("That sheet doesn't exist.")?.get_or_init(|| {
            let mut wb = self.workbook.lock();
            let (Some(Sheets::Xlsx(x)), Some(path)) = (wb.as_mut(), self.path.as_deref()) else {
                return Looks::default();
            };
            let name = &self.names[sheet];
            Looks {
                styles: x.worksheet_style(name).ok(),
                merges: x
                    .merge_cells_by_sheet_name(name)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|d| [d.start.0 as usize, d.start.1 as usize, d.end.0 as usize, d.end.1 as usize])
                    .collect(),
                rules: xlsx_parts::conditional_rules(path, name),
            }
        });
        Ok(SheetStyles { merges: looks.merges.clone(), styled: looks.styles.is_some() || !looks.rules.is_empty() })
    }

    pub fn cells(&self, sheet: usize, rows: std::ops::Range<usize>, cols: std::ops::Range<usize>) -> Result<Cells, String> {
        let grid = self.grid(sheet)?;
        let looks = self.looks.get(sheet).and_then(OnceLock::get);
        let (look, rules) = match looks {
            Some(l) => (l.styles.as_ref(), l.rules.as_slice()),
            None => (None, &[][..]),
        };
        let mut styles = Vec::new();
        let values = rows
            .map(|r| {
                cols.clone()
                    .map(|c| match grid {
                        Grid::Cells { range, origin } => {
                            let Some(cell) = range.get((r.wrapping_sub(origin.0), c.wrapping_sub(origin.1))).filter(|_| r >= origin.0 && c >= origin.1) else {
                                return String::new();
                            };
                            let style = look.and_then(|s| s.get((r as u32, c as u32)));
                            let (text, tag) = style
                                .and_then(|st| st.number_format.as_ref())
                                .and_then(|nf| number(cell).and_then(|v| numfmt::format(v, &nf.format_code)))
                                .map(|(t, tag)| (t, tag.map(str::to_owned)))
                                .unwrap_or_else(|| (format_cell(cell), None));
                            let rule = number(cell).and_then(|v| rules.iter().find(|rule| rule.applies(r, c, v)));
                            if let Some(look) = describe(r, c, style, tag, rule) {
                                styles.push(look);
                            }
                            text
                        }
                        Grid::Text(rows) => rows.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default(),
                    })
                    .collect()
            })
            .collect();
        Ok(Cells { values, styles })
    }
}

fn number(cell: &Data) -> Option<f64> {
    match cell {
        Data::Float(f) => Some(*f),
        Data::Int(i) => Some(*i as f64),
        _ => None,
    }
}

fn hex(c: &calamine::Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.red, c.green, c.blue)
}

/// The visible style of a cell, or None when it's plain black text on white.
fn describe(r: usize, c: usize, style: Option<&Style>, format_color: Option<String>, rule: Option<&xlsx_parts::Rule>) -> Option<CellStyle> {
    let font = style.and_then(|s| s.font.as_ref());
    let color = rule
        .and_then(|ru| ru.color.clone())
        .or(format_color)
        .or_else(|| font.and_then(|f| f.color.as_ref()).map(hex).filter(|h| h != "#000000"));
    let fill = rule.and_then(|ru| ru.fill.clone()).or_else(|| {
        style
            .and_then(|s| s.fill.as_ref())
            .filter(|f| f.pattern != calamine::FillPattern::None)
            .and_then(|f| f.foreground_color.as_ref())
            .map(hex)
            .filter(|h| h != "#ffffff")
    });
    let bold = font.is_some_and(|f| f.weight == calamine::FontWeight::Bold);
    (color.is_some() || fill.is_some() || bold).then_some(CellStyle { r, c, color, fill, bold })
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
        assert_eq!(xlsx_parts::frozen_panes(&path, &["A".into(), "B".into()]).unwrap(), vec![(1, 2), (0, 0)]);
    }

    #[test]
    fn csv_detects_semicolons_and_keeps_quoted_newlines() {
        let path = std::env::temp_dir().join(format!("lumen-csv-{}.csv", std::process::id()));
        std::fs::write(&path, "name;note\nAna;\"two\nlines\"\nBo;x;extra\n").unwrap();
        let doc = SheetDoc::open_delimited(&path, false).unwrap();
        let info = doc.info(0).unwrap();
        assert_eq!((info.rows, info.cols), (3, 3));
        assert_eq!(doc.cells(0, 1..2, 0..2).unwrap().values, vec![vec!["Ana".to_string(), "two\nlines".into()]]);
    }

    #[test]
    fn xlsx_number_formats_merges_and_colours() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/numbers.xlsx");
        let doc = SheetDoc::open_workbook(&path).unwrap();
        let info = doc.info(0).unwrap();
        let raw = doc.cells(0, 0..info.rows, 0..info.cols).unwrap();
        assert!(!raw.values.iter().flatten().any(|v| v == "$125,000.50"), "values come plain before styles load");
        let looks = doc.styles(0).unwrap();
        assert!(looks.styled);
        assert!(looks.merges.contains(&[0, 0, 0, 3]), "{:?}", looks.merges);
        let cells = doc.cells(0, 0..info.rows, 0..info.cols).unwrap();
        let all: Vec<&String> = cells.values.iter().flatten().collect();
        assert!(all.iter().any(|v| v.as_str() == "$125,000.50"), "{all:?}");
        assert!(all.iter().any(|v| v.as_str() == "12.40%"), "{all:?}");
        assert!(cells.styles.iter().any(|s| s.color.as_deref() == Some("#9c0006")), "the conditional red on negatives");
    }
}
