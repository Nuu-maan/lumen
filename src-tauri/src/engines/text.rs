use memmap2::Mmap;
use parking_lot::RwLock;
use serde::Serialize;
use std::borrow::Cow;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use super::highlight::{self, LineTokens};

const MAX_LINE_CHARS: usize = 10_000;
const INDEX_BATCH: usize = 1 << 16;

enum Bytes {
    Mapped(Mmap),
    Owned(Vec<u8>),
}

impl std::ops::Deref for Bytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Bytes::Mapped(m) => m,
            Bytes::Owned(v) => v,
        }
    }
}

pub struct TextDoc {
    bytes: Bytes,
    starts: RwLock<Vec<usize>>,
    indexed: AtomicBool,
    tokens: OnceLock<Option<Vec<LineTokens>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lines {
    pub start: usize,
    pub lines: Vec<String>,
    pub total: usize,
    pub complete: bool,
    pub tokens: Option<Vec<LineTokens>>,
    pub highlighting: bool,
}

fn decode_utf16(data: &[u8], big_endian: bool) -> Vec<u8> {
    let units = data[2..].chunks_exact(2).map(|c| if big_endian { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) });
    char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect::<String>().into_bytes()
}

impl TextDoc {
    pub fn open(path: &Path) -> std::io::Result<Arc<Self>> {
        let file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        // shortcut: the file is mapped, so edits made while it is open can show torn lines; copy on open if that matters.
        let map = if len == 0 { Bytes::Owned(Vec::new()) } else { Bytes::Mapped(unsafe { Mmap::map(&file)? }) };
        let bytes = match &map[..] {
            [0xFF, 0xFE, ..] => Bytes::Owned(decode_utf16(&map, false)),
            [0xFE, 0xFF, ..] => Bytes::Owned(decode_utf16(&map, true)),
            _ => map,
        };
        let doc = Arc::new(TextDoc { bytes, starts: RwLock::new(vec![0]), indexed: AtomicBool::new(false), tokens: OnceLock::new() });
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let worker = doc.clone();
        std::thread::Builder::new().name("text-index".into()).spawn(move || {
            worker.build_index();
            let tokens = (worker.bytes.len() <= highlight::MAX_BYTES).then(|| {
                let text = worker.full_text();
                highlight::highlight(&text, &extension, text.lines().next().unwrap_or(""))
            });
            let _ = worker.tokens.set(tokens.flatten());
        })?;
        Ok(doc)
    }

    fn build_index(&self) {
        let data = &self.bytes[..];
        let mut batch = Vec::with_capacity(INDEX_BATCH);
        for nl in memchr::memchr_iter(b'\n', data) {
            if nl + 1 < data.len() {
                batch.push(nl + 1);
            }
            if batch.len() == INDEX_BATCH {
                self.starts.write().append(&mut batch);
            }
        }
        self.starts.write().append(&mut batch);
        self.indexed.store(true, Ordering::Release);
    }

    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    pub fn estimated_lines(&self) -> usize {
        let starts = self.starts.read();
        let known = starts.len();
        if self.indexed.load(Ordering::Acquire) || known < 2 {
            return known.max(1);
        }
        let covered = *starts.last().unwrap();
        ((known as f64) * (self.bytes.len() as f64) / (covered.max(1) as f64)) as usize
    }

    fn line_starts(&self, start: usize, count: usize) -> Vec<usize> {
        {
            let starts = self.starts.read();
            if start + count < starts.len() || self.indexed.load(Ordering::Acquire) {
                let end = (start + count + 1).min(starts.len());
                return starts.get(start.min(end)..end).map(<[usize]>::to_vec).unwrap_or_default();
            }
        }
        let starts = self.starts.read();
        let mut out: Vec<usize> = starts.get(start..).map(<[usize]>::to_vec).unwrap_or_default();
        let mut line = starts.len() - 1;
        let mut pos = *starts.last().unwrap();
        drop(starts);
        while out.len() < count + 1 {
            match memchr::memchr(b'\n', &self.bytes[pos..]) {
                Some(i) if pos + i + 1 < self.bytes.len() => {
                    pos += i + 1;
                    line += 1;
                    if line >= start {
                        out.push(pos);
                    }
                }
                _ => break,
            }
        }
        out
    }

    pub fn lines(&self, start: usize, count: usize) -> Lines {
        let starts = self.line_starts(start, count);
        let data = &self.bytes[..];
        let lines = starts
            .iter()
            .take(count)
            .enumerate()
            .map(|(i, &s)| {
                let end = starts.get(i + 1).copied().unwrap_or(data.len());
                let raw = &data[s..end];
                let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
                let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
                let text = String::from_utf8_lossy(raw);
                let text = if start == 0 && i == 0 { text.trim_start_matches('\u{feff}').to_owned().into() } else { text };
                truncate(text)
            })
            .collect();
        let tokens = self.tokens.get().and_then(|t| t.as_ref()).map(|t| t.iter().skip(start).take(count).cloned().collect());
        Lines {
            start,
            lines,
            total: self.estimated_lines(),
            complete: self.indexed.load(Ordering::Acquire),
            tokens,
            highlighting: self.tokens.get().is_none() && self.bytes.len() <= highlight::MAX_BYTES,
        }
    }

    pub fn full_text(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.bytes)
    }

    pub fn search(&self, needle: &str, limit: usize) -> Vec<(usize, usize)> {
        let Ok(re) = regex::bytes::RegexBuilder::new(&regex::escape(needle)).case_insensitive(true).build() else {
            return Vec::new();
        };
        if needle.is_empty() {
            return Vec::new();
        }
        while !self.indexed.load(Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let starts = self.starts.read();
        let data = &self.bytes[..];
        re.find_iter(data)
            .take(limit)
            .map(|m| {
                let line = starts.partition_point(|&s| s <= m.start()) - 1;
                (line, String::from_utf8_lossy(&data[starts[line]..m.start()]).chars().count())
            })
            .collect()
    }
}

fn truncate(text: Cow<'_, str>) -> String {
    match text.char_indices().nth(MAX_LINE_CHARS) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_from(name: &str, bytes: &[u8]) -> Arc<TextDoc> {
        let path = std::env::temp_dir().join(format!("lumen-text-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let doc = TextDoc::open(&path).unwrap();
        while !doc.indexed.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        doc
    }

    #[test]
    fn lines_handle_crlf_and_missing_trailing_newline() {
        let doc = doc_from("crlf.txt", b"one\r\ntwo\r\nthree");
        let l = doc.lines(0, 10);
        assert_eq!(l.lines, ["one", "two", "three"]);
        assert_eq!(l.total, 3);
        assert_eq!(doc.lines(1, 1).lines, ["two"]);
        assert!(doc.lines(5, 2).lines.is_empty());
    }

    #[test]
    fn small_code_files_get_tokens() {
        let doc = doc_from("hl.rs", b"fn main() {}\n");
        while doc.tokens.get().is_none() {
            std::thread::yield_now();
        }
        let l = doc.lines(0, 5);
        assert!(!l.highlighting);
        assert!(l.tokens.unwrap()[0].iter().any(|&(_, c)| c == 5));
    }

    #[test]
    fn utf16_files_are_decoded() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in "héllo\nwörld".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(doc_from("u16.txt", &bytes).lines(0, 5).lines, ["héllo", "wörld"]);
    }

    #[test]
    fn search_is_case_insensitive_and_reports_columns() {
        let doc = doc_from("search.txt", b"alpha\nBeta beta\ngamma\n");
        assert_eq!(doc.search("beta", 10), [(1, 0), (1, 5)]);
        assert_eq!(doc.search("ö", 10), []);
    }
}
