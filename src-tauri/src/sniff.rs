use crate::formats::{by_ext, by_id, Format};
use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;

#[derive(Debug)]
pub struct Sniffed {
    pub format: &'static Format,
    pub encrypted: bool,
}

pub fn sniff(path: &Path) -> std::io::Result<Option<Sniffed>> {
    let mut file = File::open(path)?;
    let mut head = vec![0u8; 8192];
    let n = read_up_to(&mut file, &mut head)?;
    head.truncate(n);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let ext_format = by_ext(&ext);
    let plain = |id| Some(Sniffed { format: by_id(id), encrypted: false });

    let magic = if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        file.rewind()?;
        sniff_zip(file, ext_format).map(|id| Sniffed { format: by_id(id), encrypted: false })
    } else if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        file.rewind()?;
        sniff_cfb(file, ext_format)
    } else {
        sniff_bytes(&head, ext_format).and_then(plain)
    };
    Ok(magic.or_else(|| ext_format.filter(|_| looks_like_text(&head)).map(|f| Sniffed { format: f, encrypted: false })))
}

fn read_up_to(file: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match file.read(&mut buf[total..])? {
            0 => break,
            n => total += n,
        }
    }
    Ok(total)
}

fn find(hay: &[u8], needle: &[u8]) -> bool {
    memchr::memmem::find(hay, needle).is_some()
}

fn sniff_bytes(head: &[u8], ext: Option<&'static Format>) -> Option<&'static str> {
    let starts = |m: &[u8]| head.starts_with(m);
    if find(&head[..head.len().min(1024)], b"%PDF-") {
        return Some("pdf");
    }
    if starts(b"{\\rtf") {
        return Some("rtf");
    }
    if starts(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if starts(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpeg");
    }
    if starts(b"GIF87a") || starts(b"GIF89a") {
        return Some("gif");
    }
    if starts(b"RIFF") && head.get(8..12) == Some(b"WEBP") {
        return Some("webp");
    }
    if head.get(4..8) == Some(b"ftyp") {
        return match head.get(8..12) {
            Some(b"avif") | Some(b"avis") => Some("avif"),
            Some(b"heic") | Some(b"heix") | Some(b"mif1") | Some(b"msf1") | Some(b"hevc") => Some("heic"),
            _ => None,
        };
    }
    if starts(b"II*\0") || starts(b"MM\0*") {
        return Some("tiff");
    }
    if starts(b"BM") && head.len() > 14 && ext.map(|f| f.id) == Some("bmp") {
        return Some("bmp");
    }
    if starts(&[0, 0, 1, 0]) || starts(&[0, 0, 2, 0]) {
        return Some("ico");
    }
    if starts(&[0xFF, b'W', b'P', b'C']) {
        return Some("wpd");
    }
    if head.get(60..68) == Some(b"BOOKMOBI") || head.get(60..68) == Some(b"TEXtREAd") {
        return Some("mobi");
    }
    if !looks_like_text(head) {
        return None;
    }
    sniff_text(head, ext)
}

fn sniff_text(head: &[u8], ext: Option<&'static Format>) -> Option<&'static str> {
    let text = String::from_utf8_lossy(&head[..head.len().min(4096)]).to_ascii_lowercase();
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with("<?xml") || trimmed.starts_with("<svg") || trimmed.starts_with("<!doctype svg") {
        if text.contains("<svg") {
            return Some("svg");
        }
        if text.contains("<fictionbook") {
            return Some("fb2");
        }
        if text.contains("<html") {
            return Some("html");
        }
        return ext.map(|f| f.id).or(Some("xml"));
    }
    if trimmed.starts_with("<!doctype html") || trimmed.starts_with("<html") {
        return Some("html");
    }
    let has_header = |h: &str| text.lines().take(40).any(|l| l.starts_with(h));
    if has_header("mime-version:") && text.contains("multipart/related") {
        return Some("mht");
    }
    let mail_headers = ["from:", "to:", "subject:", "date:", "received:", "message-id:", "return-path:"];
    if ext.is_none_or(|f| f.id == "eml") && mail_headers.iter().filter(|h| has_header(h)).count() >= 3 {
        return Some("eml");
    }
    ext.map(|f| f.id).or(Some("txt"))
}

pub fn looks_like_text(head: &[u8]) -> bool {
    if head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]) {
        return true;
    }
    let sample = &head[..head.len().min(4096)];
    if sample.contains(&0) {
        return false;
    }
    let cut = sample.len().saturating_sub(4);
    match std::str::from_utf8(sample) {
        Ok(_) => true,
        Err(e) => e.valid_up_to() >= cut || e.error_len().is_none(),
    }
}

fn sniff_zip(file: File, ext: Option<&'static Format>) -> Option<&'static str> {
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).ok()?;
    if let Ok(mut entry) = zip.by_name("mimetype") {
        let mut mime = String::new();
        let _ = entry.by_ref().take(128).read_to_string(&mut mime);
        let mime = mime.trim();
        let id = match mime {
            "application/epub+zip" => Some("epub"),
            m if m.starts_with("application/vnd.oasis.opendocument.text") => Some("odt"),
            m if m.starts_with("application/vnd.oasis.opendocument.spreadsheet") => Some("ods"),
            m if m.starts_with("application/vnd.oasis.opendocument.presentation") => Some("odp"),
            _ => None,
        };
        if id.is_some() {
            return id;
        }
    }
    let names: Vec<String> = zip.file_names().take(4000).filter_map(|n| n.ok().map(|n| n.into_owned())).collect();
    let any = |p: &str| names.iter().any(|n| n.starts_with(p));
    let ext_id = ext.map(|f| f.id);
    if names.iter().any(|n| n == "[Content_Types].xml") {
        let ooxml = if any("word/") {
            Some("docx")
        } else if any("xl/") {
            if names.iter().any(|n| n.ends_with(".bin") && n.starts_with("xl/worksheets/")) {
                Some("xlsb")
            } else {
                Some("xlsx")
            }
        } else if any("ppt/") {
            Some("pptx")
        } else if any("visio/") {
            Some("vsdx")
        } else if any("Documents/") || names.iter().any(|n| n.ends_with(".fdseq")) {
            Some("xps")
        } else {
            None
        };
        if ooxml.is_some() {
            return ooxml;
        }
    }
    if names.iter().any(|n| n.ends_with(".fdseq") || n.ends_with(".fdoc")) {
        return Some("xps");
    }
    if any("Index/") || names.iter().any(|n| n == "Index.zip") {
        return ext_id.filter(|id| ["pages", "numbers", "key"].contains(id)).or(Some("pages"));
    }
    let images = names.iter().filter(|n| is_image_name(n)).count();
    if ext_id == Some("cbz") || (images > 0 && images * 10 >= names.len() * 8) {
        return Some("cbz");
    }
    Some("zip")
}

fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".jpg", ".jpeg", ".png", ".gif", ".webp", ".bmp"].iter().any(|e| lower.ends_with(e))
}

fn sniff_cfb(file: File, ext: Option<&'static Format>) -> Option<Sniffed> {
    let comp = cfb::CompoundFile::open(file).ok()?;
    let has = |p: &str| comp.exists(p);
    let ext_id = ext.map(|f| f.id);
    let id = if has("/EncryptionInfo") && has("/EncryptedPackage") {
        let id = ext_id.filter(|id| ["docx", "xlsx", "xlsb", "pptx"].contains(id)).unwrap_or("docx");
        return Some(Sniffed { format: by_id(id), encrypted: true });
    } else if has("/WordDocument") {
        "doc"
    } else if has("/Workbook") || has("/Book") {
        "xls"
    } else if has("/PowerPoint Document") {
        "ppt"
    } else if has("/VisioDocument") {
        "vsd"
    } else if has("/Quill") || has("/Contents") && ext_id == Some("pub") {
        "pub"
    } else if has("/FileHeader") && has("/BodyText") {
        "hwp"
    } else if comp.read_root_storage().any(|e| e.name().starts_with("__substg1.0_")) {
        "msg"
    } else {
        ext_id?
    };
    Some(Sniffed { format: by_id(id), encrypted: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sniff_bytes_as(name: &str, bytes: &[u8]) -> Option<&'static str> {
        let dir = std::env::temp_dir().join(format!("lumen-sniff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        sniff(&path).unwrap().map(|s| s.format.id)
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut buf);
        for (name, data) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
        buf.into_inner()
    }

    #[test]
    fn magic_beats_extension() {
        assert_eq!(sniff_bytes_as("a.txt", b"%PDF-1.7\n..."), Some("pdf"));
        assert_eq!(sniff_bytes_as("a.doc", b"{\\rtf1\\ansi hello}"), Some("rtf"));
        assert_eq!(sniff_bytes_as("photo.jpg", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), Some("png"));
        assert_eq!(sniff_bytes_as("x", b"\0\0\0\x1cftypavif\0\0\0\0"), Some("avif"));
    }

    #[test]
    fn text_formats_use_content_then_extension() {
        assert_eq!(sniff_bytes_as("x.xml", b"<?xml version=\"1.0\"?><svg xmlns=\"\"/>"), Some("svg"));
        assert_eq!(sniff_bytes_as("page", b"<!DOCTYPE html><html></html>"), Some("html"));
        assert_eq!(sniff_bytes_as("main.rs", b"fn main() {}\n"), Some("code"));
        assert_eq!(sniff_bytes_as("notes", b"just some words\n"), Some("txt"));
        assert_eq!(
            sniff_bytes_as("mail", b"From: a@b.c\nTo: d@e.f\nSubject: hi\nDate: now\n\nbody"),
            Some("eml")
        );
    }

    #[test]
    fn zip_containers_are_told_apart() {
        let docx = zip_with(&[("[Content_Types].xml", b"<Types/>"), ("word/document.xml", b"<w/>")]);
        assert_eq!(sniff_bytes_as("report.bin", &docx), Some("docx"));
        let xlsx = zip_with(&[("[Content_Types].xml", b"<Types/>"), ("xl/workbook.xml", b"<w/>")]);
        assert_eq!(sniff_bytes_as("a.docx", &xlsx), Some("xlsx"));
        let epub = zip_with(&[("mimetype", b"application/epub+zip"), ("OEBPS/a.xhtml", b"")]);
        assert_eq!(sniff_bytes_as("book.zip", &epub), Some("epub"));
        let odt = zip_with(&[("mimetype", b"application/vnd.oasis.opendocument.text")]);
        assert_eq!(sniff_bytes_as("a", &odt), Some("odt"));
        let comic = zip_with(&[("01.jpg", b""), ("02.jpg", b""), ("03.png", b"")]);
        assert_eq!(sniff_bytes_as("a.zip", &comic), Some("cbz"));
        let plain = zip_with(&[("readme.txt", b"hi"), ("src/main.rs", b"")]);
        assert_eq!(sniff_bytes_as("a.zip", &plain), Some("zip"));
    }

    #[test]
    fn binary_without_known_magic_is_unsupported() {
        assert_eq!(sniff_bytes_as("blob.bin", &[0, 1, 2, 3, 0, 0, 7]), None);
    }
}
