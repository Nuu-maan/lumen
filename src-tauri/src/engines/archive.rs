use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_EXTRACT: u64 = 2 << 30;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub index: usize,
    pub path: String,
    pub size: u64,
    pub packed: u64,
    pub dir: bool,
}

pub fn list(path: &Path) -> Result<Vec<Entry>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| format!("This archive is damaged or not a ZIP file ({e})."))?;
    let mut entries = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let Ok(e) = zip.by_index_raw(i) else { continue };
        entries.push(Entry { index: i, path: e.name().map(|n| n.into_owned()).unwrap_or_default(), size: e.size(), packed: e.compressed_size(), dir: e.is_dir() });
    }
    Ok(entries)
}

pub fn extract(path: &Path, index: usize, into: &Path) -> Result<PathBuf, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    let mut entry = zip.by_index(index).map_err(|e| match e {
        zip::result::ZipError::UnsupportedArchive(m) => format!("This entry can't be extracted: {m}."),
        e => e.to_string(),
    })?;
    let name = entry.enclosed_name().and_then(|p| p.file_name().map(PathBuf::from)).ok_or("This entry has an unsafe path and was not extracted.")?;
    std::fs::create_dir_all(into).map_err(|e| e.to_string())?;
    let target = into.join(name);
    let mut out = std::fs::File::create(&target).map_err(|e| e.to_string())?;
    let copied = std::io::copy(&mut (&mut entry).take(MAX_EXTRACT), &mut out).map_err(|e| e.to_string())?;
    if copied >= MAX_EXTRACT {
        drop(out);
        let _ = std::fs::remove_file(&target);
        return Err("This entry is larger than 2 GB and was not extracted.".into());
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn lists_and_extracts_without_escaping_target_dir() {
        let dir = std::env::temp_dir().join(format!("lumen-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("a.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        w.start_file("docs/readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(b"hello").unwrap();
        w.start_file("../../evil.txt", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(b"x").unwrap();
        w.finish().unwrap();
        let entries = list(&zip_path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "docs/readme.txt");
        let out = extract(&zip_path, 0, &dir.join("out")).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "hello");
        assert!(extract(&zip_path, 1, &dir.join("out")).is_err());
    }
}
