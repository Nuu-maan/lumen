use super::markup::escape;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Serialize;
use std::io::Read;
use std::path::Path;

const PREVIEW_CSS: &str = "<style>
:root{color-scheme:light}
body{margin:0;padding:32px 16px;background:#e9e7e3;font:15px/1.55 Calibri,Carlito,'Segoe UI',system-ui,sans-serif;color:#1d1d1f}
.paper{max-width:794px;margin:0 auto 24px;padding:72px 80px;background:#fff;box-shadow:0 1px 3px rgb(0 0 0/.15)}
h1,h2,h3,h4{line-height:1.25;margin:1.2em 0 .5em}
p{margin:0 0 .6em;white-space:pre-wrap}
table{border-collapse:collapse;margin:0 0 1em;width:100%}
td{border:1px solid #c8c8c8;padding:4px 6px;vertical-align:top}
.slide{aspect-ratio:16/9;padding:48px 64px;overflow:hidden}
.slide h2{margin-top:0}
.notes{max-width:794px;margin:-12px auto 24px;padding:12px 16px;background:#f6f5f2;border-left:3px solid #b9b4aa;font-size:14px}
</style>";

#[derive(Serialize)]
pub struct Fast {
    pub html: String,
    pub notes: Vec<String>,
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut entry = zip.by_name(name).ok()?;
    let mut buf = Vec::with_capacity(entry.size().min(64 << 20) as usize);
    entry.by_ref().take(256 << 20).read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn attr(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
    e.attributes().flatten().find(|a| local(a.key.as_ref()) == key).map(|a| a.value.to_string())
}

fn push_entity(out: &mut String, name: &str) {
    let c = match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        n => n
            .strip_prefix("#x")
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .or_else(|| n.strip_prefix('#').and_then(|d| d.parse().ok()))
            .and_then(char::from_u32),
    };
    out.extend(c);
}

fn wrap(body: &str) -> String {
    super::markup::sandboxed(&format!("<!doctype html><html><head><meta charset=\"utf-8\">{PREVIEW_CSS}</head><body>{body}</body></html>"))
}

fn heading_level(style: &str) -> Option<u8> {
    let s = style.to_ascii_lowercase().replace(' ', "");
    if s == "title" {
        return Some(1);
    }
    s.strip_prefix("heading").and_then(|n| n.parse::<u8>().ok()).map(|n| n.clamp(1, 6))
}

pub fn docx(path: &Path) -> Option<Fast> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let xml = read_entry(&mut zip, "word/document.xml")?;
    let mut reader = Reader::from_reader(xml.as_slice());
    let mut out = String::from("<div class=\"paper\">");
    let mut para = String::new();
    let mut style = String::new();
    let mut in_text = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(e) | Event::Empty(e) if local(e.name().as_ref()) == "pStyle" => style = attr(&e, "val").unwrap_or_default(),
            Event::Start(e) => match local(e.name().as_ref()) {
                "t" => in_text = true,
                "tbl" => out.push_str("<table>"),
                "tr" => out.push_str("<tr>"),
                "tc" => out.push_str("<td>"),
                "p" => {
                    para.clear();
                    style.clear();
                }
                _ => {}
            },
            Event::Empty(e) => match local(e.name().as_ref()) {
                "tab" => para.push('\t'),
                "br" | "cr" => para.push('\n'),
                _ => {}
            },
            Event::Text(t) if in_text => para.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if in_text => push_entity(&mut para, &r.xml10_content()),
            Event::End(e) => match local(e.name().as_ref()) {
                "t" => in_text = false,
                "tbl" => out.push_str("</table>"),
                "tr" => out.push_str("</tr>"),
                "tc" => out.push_str("</td>"),
                "p" => match heading_level(&style) {
                    Some(h) => out.push_str(&format!("<h{h}>{}</h{h}>", escape(&para))),
                    None => out.push_str(&format!("<p>{}</p>", if para.is_empty() { "&nbsp;".into() } else { escape(&para) })),
                },
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    out.push_str("</div>");
    Some(Fast { html: wrap(&out), notes: Vec::new() })
}

fn paragraphs(xml: &[u8]) -> Vec<String> {
    let mut reader = Reader::from_reader(xml);
    let mut paras = Vec::new();
    let mut current = String::new();
    let mut in_text = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == "t" => in_text = true,
            Ok(Event::Text(t)) if in_text => current.push_str(&t.xml10_content()),
            Ok(Event::GeneralRef(r)) if in_text => push_entity(&mut current, &r.xml10_content()),
            Ok(Event::End(e)) => match local(e.name().as_ref()) {
                "t" => in_text = false,
                "p" => {
                    if !current.trim().is_empty() {
                        paras.push(std::mem::take(&mut current));
                    }
                    current.clear();
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    paras
}

fn numbered(names: impl Iterator<Item = String>, prefix: &str) -> Vec<(u32, String)> {
    let mut found: Vec<(u32, String)> = names
        .filter_map(|n| {
            let num = n.strip_prefix(prefix)?.strip_suffix(".xml")?.parse().ok()?;
            Some((num, n))
        })
        .collect();
    found.sort();
    found
}

pub fn pptx(path: &Path) -> Option<Fast> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let names: Vec<String> = zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect();
    let slides = numbered(names.into_iter(), "ppt/slides/slide");
    let mut out = String::new();
    let mut notes = Vec::new();
    for (i, (num, name)) in slides.iter().enumerate() {
        let paras = read_entry(&mut zip, name).map(|x| paragraphs(&x)).unwrap_or_default();
        out.push_str(&format!("<div class=\"paper slide\" aria-label=\"Slide {}\">", i + 1));
        for (k, p) in paras.iter().enumerate() {
            if k == 0 {
                out.push_str(&format!("<h2>{}</h2>", escape(p)));
            } else {
                out.push_str(&format!("<p>{}</p>", escape(p)));
            }
        }
        out.push_str("</div>");
        let rels = read_entry(&mut zip, &format!("ppt/slides/_rels/slide{num}.xml.rels")).unwrap_or_default();
        let rels = String::from_utf8_lossy(&rels);
        let note = rels
            .split("Target=\"")
            .skip(1)
            .filter_map(|s| s.split('"').next())
            .find(|t| t.contains("notesSlide"))
            .and_then(|t| read_entry(&mut zip, &format!("ppt/notesSlides/{}", t.rsplit('/').next().unwrap_or(t))))
            .map(|x| {
                paragraphs(&x)
                    .into_iter()
                    .filter(|p| p.trim().parse::<u32>().is_err())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if !note.is_empty() {
            out.push_str(&format!("<div class=\"notes\" aria-label=\"Notes for slide {}\"><b>Notes</b><p>{}</p></div>", i + 1, escape(&note)));
        }
        notes.push(note);
    }
    Some(Fast { html: wrap(&out), notes })
}

const TAB: char = 9 as char;
const NEWLINE: char = 10 as char;

/// The XML of an OpenDocument file: content.xml inside the zip, or the whole file for flat .fodt/.fodp.
fn odf_content(path: &Path) -> Option<Vec<u8>> {
    match zip::ZipArchive::new(std::fs::File::open(path).ok()?) {
        Ok(mut zip) => read_entry(&mut zip, "content.xml"),
        Err(_) => std::fs::read(path).ok(),
    }
}

/// Text inside text:p and text:h, with the spacing elements OpenDocument uses instead of whitespace.
struct OdfText {
    para: String,
    depth: usize,
}

impl OdfText {
    fn new() -> Self {
        OdfText { para: String::new(), depth: 0 }
    }

    fn empty(&mut self, name: &str, e: &quick_xml::events::BytesStart) {
        if self.depth == 0 {
            return;
        }
        match name {
            "s" => {
                let n = attr(e, "c").and_then(|c| c.parse().ok()).unwrap_or(1usize);
                self.para.extend(std::iter::repeat_n(' ', n.min(64)));
            }
            "tab" => self.para.push(TAB),
            "line-break" => self.para.push(NEWLINE),
            _ => {}
        }
    }
}

/// OpenDocument text (ODT): headings, paragraphs, lists and tables.
pub fn odt(path: &Path) -> Option<Fast> {
    let xml = odf_content(path)?;
    let mut reader = Reader::from_reader(xml.as_slice());
    let mut out = String::from(r#"<div class="paper">"#);
    let mut text = OdfText::new();
    let mut level: Option<u8> = None;
    let mut in_body = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(e) => match local(e.name().as_ref()) {
                "text" if !in_body => in_body = true,
                "h" | "p" if in_body => {
                    if text.depth == 0 {
                        text.para.clear();
                        level = (local(e.name().as_ref()) == "h")
                            .then(|| attr(&e, "outline-level").and_then(|v| v.parse::<u8>().ok()).unwrap_or(1).clamp(1, 6));
                    }
                    text.depth += 1;
                }
                "table" if in_body => out.push_str("<table>"),
                "table-row" if in_body => out.push_str("<tr>"),
                "table-cell" if in_body => out.push_str("<td>"),
                _ => {}
            },
            Event::Empty(e) => {
                let name = local(e.name().as_ref()).to_owned();
                if in_body && text.depth == 0 && (name == "p" || name == "h") {
                    out.push_str("<p>&nbsp;</p>");
                }
                text.empty(&name, &e);
            }
            Event::Text(t) if text.depth > 0 => text.para.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if text.depth > 0 => push_entity(&mut text.para, &r.xml10_content()),
            Event::End(e) => match local(e.name().as_ref()) {
                "h" | "p" if text.depth > 0 => {
                    text.depth -= 1;
                    if text.depth == 0 {
                        match level {
                            Some(h) => out.push_str(&format!("<h{h}>{}</h{h}>", escape(&text.para))),
                            None if text.para.is_empty() => out.push_str("<p>&nbsp;</p>"),
                            None => out.push_str(&format!("<p>{}</p>", escape(&text.para))),
                        }
                    }
                }
                "table" if in_body => out.push_str("</table>"),
                "table-row" if in_body => out.push_str("</tr>"),
                "table-cell" if in_body => out.push_str("</td>"),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    out.push_str("</div>");
    Some(Fast { html: wrap(&out), notes: Vec::new() })
}

/// OpenDocument presentation (ODP): each slide's text, and its speaker notes.
pub fn odp(path: &Path) -> Option<Fast> {
    let xml = odf_content(path)?;
    let mut reader = Reader::from_reader(xml.as_slice());
    let mut out = String::new();
    let mut notes = Vec::new();
    let mut slide: Vec<String> = Vec::new();
    let mut note: Vec<String> = Vec::new();
    let mut in_notes = false;
    let mut text = OdfText::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(e) => match local(e.name().as_ref()) {
                "page" => {
                    slide.clear();
                    note.clear();
                }
                "notes" => in_notes = true,
                "p" | "h" => {
                    if text.depth == 0 {
                        text.para.clear();
                    }
                    text.depth += 1;
                }
                _ => {}
            },
            Event::Empty(e) => {
                let name = local(e.name().as_ref()).to_owned();
                text.empty(&name, &e);
            }
            Event::Text(t) if text.depth > 0 => text.para.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if text.depth > 0 => push_entity(&mut text.para, &r.xml10_content()),
            Event::End(e) => match local(e.name().as_ref()) {
                "p" | "h" if text.depth > 0 => {
                    text.depth -= 1;
                    if text.depth == 0 && !text.para.trim().is_empty() {
                        let para = std::mem::take(&mut text.para);
                        if in_notes {
                            note.push(para)
                        } else {
                            slide.push(para)
                        }
                    }
                }
                "notes" => in_notes = false,
                "page" => {
                    let i = notes.len() + 1;
                    out.push_str(&format!(r#"<div class="paper slide" aria-label="Slide {i}">"#));
                    for (k, p) in slide.iter().enumerate() {
                        let tag = if k == 0 { "h2" } else { "p" };
                        out.push_str(&format!("<{tag}>{}</{tag}>", escape(p)));
                    }
                    out.push_str("</div>");
                    let joined = note.join(&NEWLINE.to_string());
                    if !joined.is_empty() {
                        out.push_str(&format!(
                            r#"<div class="notes" aria-label="Notes for slide {i}"><b>Notes</b><p>{}</p></div>"#,
                            escape(&joined)
                        ));
                    }
                    notes.push(joined);
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    (!notes.is_empty()).then(|| Fast { html: wrap(&out), notes })
}

/// Pages, Keynote and Numbers files carry a preview image of their first page or slide.
pub fn iwork(path: &Path) -> Option<Fast> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let (bytes, mime) = ["preview.jpg", "QuickLook/Preview.jpg", "QuickLook/Thumbnail.jpg"]
        .iter()
        .find_map(|name| read_entry(&mut zip, name))
        .map(|b| (b, "image/jpeg"))
        .or_else(|| read_entry(&mut zip, "QuickLook/Thumbnail.png").map(|b| (b, "image/png")))?;
    let img = format!(
        r#"<div class="paper"><img alt="Preview of the first page" style="display:block;max-width:100%;margin:0 auto" src="data:{mime};base64,{}"></div>"#,
        super::email::base64(&bytes)
    );
    Some(Fast { html: wrap(&img), notes: Vec::new() })
}

/// RTF groups whose text isn't document content: tables of fonts and colours, metadata, pictures,
/// headers and the instruction half of fields.
const RTF_SKIP: &[&str] = &[
    "fonttbl", "colortbl", "stylesheet", "info", "pict", "header", "headerl", "headerr", "headerf", "footer", "footerl", "footerr",
    "footerf", "listtable", "listoverridetable", "rsidtbl", "generator", "xmlnstbl", "themedata", "colorschememapping",
    "latentstyles", "datastore", "fldinst", "object", "filetbl", "revtbl", "pgdsctbl",
];

/// Rich Text: paragraph text, with fonts, colours, pictures and field codes left out.
pub fn rtf(path: &Path) -> Option<Fast> {
    let data = std::fs::read(path).ok()?;
    if !data.starts_with(b"{\\rtf") {
        return None;
    }
    #[derive(Clone, Copy)]
    struct Group {
        skip: bool,
        uc: usize,
    }
    let mut stack = vec![Group { skip: false, uc: 1 }];
    let mut text = String::new();
    let mut fallback = 0usize;
    let mut i = 0;
    let push = |text: &mut String, fallback: &mut usize, skip: bool, ch: char| {
        if *fallback > 0 {
            *fallback -= 1;
        } else if !skip {
            text.push(ch);
        }
    };
    while i < data.len() {
        let skip = stack.last().is_some_and(|g| g.skip);
        match data[i] {
            b'{' => {
                let top = *stack.last()?;
                stack.push(top);
                i += 1;
            }
            b'}' => {
                if stack.len() > 1 {
                    stack.pop();
                }
                i += 1;
            }
            b'\\' => {
                i += 1;
                let Some(&c) = data.get(i) else { break };
                match c {
                    b'\\' | b'{' | b'}' => {
                        push(&mut text, &mut fallback, skip, c as char);
                        i += 1;
                    }
                    b'\'' => {
                        let hex = std::str::from_utf8(data.get(i + 1..i + 3)?).ok()?;
                        let byte = u8::from_str_radix(hex, 16).unwrap_or(b'?');
                        push(&mut text, &mut fallback, skip, cp1252(byte));
                        i += 3;
                    }
                    b'*' => {
                        if let Some(g) = stack.last_mut() {
                            g.skip = true;
                        }
                        i += 1;
                    }
                    b'~' => {
                        push(&mut text, &mut fallback, skip, ' ');
                        i += 1;
                    }
                    b'_' => {
                        push(&mut text, &mut fallback, skip, '-');
                        i += 1;
                    }
                    b'\r' | b'\n' => {
                        push(&mut text, &mut fallback, skip, '\n');
                        i += 1;
                    }
                    c if c.is_ascii_alphabetic() => {
                        let start = i;
                        while data.get(i).is_some_and(u8::is_ascii_alphabetic) {
                            i += 1;
                        }
                        let word = std::str::from_utf8(&data[start..i]).ok()?;
                        let num_start = i;
                        if data.get(i) == Some(&b'-') {
                            i += 1;
                        }
                        while data.get(i).is_some_and(u8::is_ascii_digit) {
                            i += 1;
                        }
                        let param: Option<i64> = std::str::from_utf8(&data[num_start..i]).ok().and_then(|n| n.parse().ok());
                        if data.get(i) == Some(&b' ') {
                            i += 1;
                        }
                        match word {
                            w if RTF_SKIP.contains(&w) => {
                                if let Some(g) = stack.last_mut() {
                                    g.skip = true;
                                }
                            }
                            "par" | "line" | "row" | "sect" | "page" => push(&mut text, &mut 0, skip, '\n'),
                            "tab" | "cell" => push(&mut text, &mut 0, skip, '\t'),
                            "uc" => {
                                if let Some(g) = stack.last_mut() {
                                    g.uc = param.unwrap_or(1).max(0) as usize;
                                }
                            }
                            "u" => {
                                let code = param.unwrap_or(63);
                                let code = if code < 0 { code + 65536 } else { code };
                                push(&mut text, &mut 0, skip, char::from_u32(code as u32).unwrap_or('?'));
                                fallback = stack.last().map_or(1, |g| g.uc);
                            }
                            "emdash" => push(&mut text, &mut fallback, skip, '—'),
                            "endash" => push(&mut text, &mut fallback, skip, '–'),
                            "bullet" => push(&mut text, &mut fallback, skip, '•'),
                            "lquote" => push(&mut text, &mut fallback, skip, '‘'),
                            "rquote" => push(&mut text, &mut fallback, skip, '’'),
                            "ldblquote" => push(&mut text, &mut fallback, skip, '“'),
                            "rdblquote" => push(&mut text, &mut fallback, skip, '”'),
                            _ => {}
                        }
                    }
                    _ => i += 1,
                }
            }
            b'\r' | b'\n' => i += 1,
            b => {
                push(&mut text, &mut fallback, skip, cp1252(b));
                i += 1;
            }
        }
    }
    let mut out = String::from(r#"<div class="paper">"#);
    for para in text.split('\n') {
        if para.trim().is_empty() {
            out.push_str("<p>&nbsp;</p>");
        } else {
            out.push_str(&format!("<p>{}</p>", escape(para)));
        }
    }
    out.push_str("</div>");
    Some(Fast { html: wrap(&out), notes: Vec::new() })
}

/// A byte in Windows-1252, the code page almost every RTF file declares.
fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–',
        '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[(b - 0x80) as usize],
        b => b as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_file(name: &str, entries: &[(&str, &str)]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("lumen-fast-{}-{name}", std::process::id()));
        let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (n, data) in entries {
            w.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(data.as_bytes()).unwrap();
        }
        w.finish().unwrap();
        path
    }

    #[test]
    fn docx_headings_paragraphs_and_tables() {
        let doc = r#"<w:document xmlns:w="w"><w:body>
            <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Intro &amp; scope</w:t></w:r></w:p>
            <w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:tab/><w:t xml:space="preserve"> world</w:t></w:r></w:p>
            <w:tbl><w:tr><w:tc><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
        </w:body></w:document>"#;
        let fast = docx(&zip_file("a.docx", &[("word/document.xml", doc)])).unwrap();
        assert!(fast.html.contains("<h1>Intro &amp; scope</h1>"), "{}", fast.html);
        assert!(fast.html.contains("<p>Hello\t world</p>"));
        assert!(fast.html.contains("<table><tr><td><p>A1</p></td></tr></table>"));
    }

    #[test]
    fn pptx_slides_in_numeric_order_with_notes() {
        let slide = |t: &str| format!(r#"<p:sld xmlns:a="a" xmlns:p="p"><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:sld>"#);
        let (s1, s2, s10) = (slide("One"), slide("Two"), slide("Ten"));
        let rel = r#"<Relationships><Relationship Target="../notesSlides/notesSlide7.xml"/></Relationships>"#;
        let note = r#"<p:notes xmlns:a="a" xmlns:p="p"><a:p><a:r><a:t>Say hi</a:t></a:r></a:p><a:p><a:r><a:t>2</a:t></a:r></a:p></p:notes>"#;
        let path = zip_file(
            "a.pptx",
            &[
                ("ppt/slides/slide10.xml", &s10),
                ("ppt/slides/slide2.xml", &s2),
                ("ppt/slides/slide1.xml", &s1),
                ("ppt/slides/_rels/slide2.xml.rels", rel),
                ("ppt/notesSlides/notesSlide7.xml", note),
            ],
        );
        let fast = pptx(&path).unwrap();
        let (one, two, ten) = (fast.html.find("One").unwrap(), fast.html.find("Two").unwrap(), fast.html.find("Ten").unwrap());
        assert!(one < two && two < ten);
        assert_eq!(fast.notes, vec!["".to_string(), "Say hi".into(), "".into()]);
        let (shown, ten) = (fast.html.find("<p>Say hi</p>").unwrap(), fast.html.find("Ten").unwrap());
        assert!(two < shown && shown < ten, "{}", fast.html);
    }

    const ODF_NS: &str = r#"xmlns:office="o" xmlns:text="t" xmlns:table="b" xmlns:draw="d" xmlns:presentation="p""#;

    #[test]
    fn odt_headings_spacing_and_tables() {
        let content = format!(
            r#"<office:document-content {ODF_NS}><office:body><office:text><text:h text:outline-level="2">Plan &amp; scope</text:h><text:p>a<text:s text:c="3"/>b<text:tab/>c</text:p><text:p/><table:table><table:table-row><table:table-cell><text:p>A1</text:p></table:table-cell></table:table-row></table:table></office:text></office:body></office:document-content>"#
        );
        let fast = odt(&zip_file("a.odt", &[("content.xml", &content)])).unwrap();
        assert!(fast.html.contains("<h2>Plan &amp; scope</h2>"), "{}", fast.html);
        assert!(fast.html.contains("a   b"), "{}", fast.html);
        assert!(fast.html.contains("<p>&nbsp;</p>"));
        assert!(fast.html.contains("<table><tr><td><p>A1</p></td></tr></table>"), "{}", fast.html);

        let flat = std::env::temp_dir().join(format!("lumen-fast-{}-flat.fodt", std::process::id()));
        std::fs::write(&flat, &content).unwrap();
        assert!(odt(&flat).unwrap().html.contains("Plan &amp; scope"));
    }

    #[test]
    fn odp_slides_and_notes() {
        let content = format!(
            r#"<office:document-content {ODF_NS}><office:body><office:presentation><draw:page><draw:frame><draw:text-box><text:p>Quarterly review</text:p><text:p>Revenue grew</text:p></draw:text-box></draw:frame><presentation:notes><draw:frame><draw:text-box><text:p>Open with the number.</text:p></draw:text-box></draw:frame></presentation:notes></draw:page><draw:page><draw:frame><draw:text-box><text:p>Regions</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"#
        );
        let fast = odp(&zip_file("a.odp", &[("content.xml", &content)])).unwrap();
        assert_eq!(fast.notes, vec!["Open with the number.".to_string(), String::new()]);
        let (title, notes, second) = (fast.html.find("Quarterly review").unwrap(), fast.html.find("Open with").unwrap(), fast.html.find("Regions").unwrap());
        assert!(title < notes && notes < second, "{}", fast.html);
    }

    #[test]
    fn iwork_preview_image() {
        let fast = iwork(&zip_file("a.key", &[("preview.jpg", "jpeg bytes")])).unwrap();
        assert!(fast.html.contains("src=\"data:image/jpeg;base64,anBlZyBieXRlcw==\""), "{}", fast.html);
        assert!(iwork(&zip_file("b.key", &[("Index/Document.iwa", "x")])).is_none());
    }

    #[test]
    fn rtf_text_without_tables_and_fields() {
        let path = std::env::temp_dir().join(format!("lumen-fast-{}-a.rtf", std::process::id()));
        let source = r#"{\rtf1\ansi\ansicpg1252\uc1{\fonttbl{\f0 Calibri;}}{\colortbl;\red255\green0\blue0;}{\*\generator Word;}
{\pard Caf\'e9 \b bold\b0  and \u8364? euro\par}
{\pard Line two\tab tabbed\par}
{\field{\*\fldinst HYPERLINK "x"}{\fldrslt Link text}}\par
{\pict\pngblip 89504e47}End \{braces\}}"#;
        std::fs::write(&path, source).unwrap();
        let fast = rtf(&path).unwrap();
        assert!(fast.html.contains("<p>Café bold and € euro</p>"), "{}", fast.html);
        assert!(fast.html.contains("<p>Line two\ttabbed</p>"), "{}", fast.html);
        assert!(fast.html.contains("Link text"));
        let body = fast.html.split(r#"<div class="paper">"#).nth(1).unwrap();
        assert!(!body.contains("Calibri") && !body.contains("HYPERLINK") && !body.contains("89504e47") && !body.contains("Word"), "{body}");
        assert!(fast.html.contains("End {braces}"), "{}", fast.html);
    }
}
