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
        notes.push(note);
    }
    Some(Fast { html: wrap(&out), notes })
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
    }
}
