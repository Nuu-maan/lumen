use crate::docs::{Doc, OpenPagedResult, Registry};
use crate::engines::text::{Lines, TextDoc};
use crate::engines::{archive, email, markup};
use crate::formats::{Engine, Format};
use crate::render;
use crate::sniff;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const INLINE_HTML_LIMIT: u64 = 64 << 20;
const FIRST_LINES: usize = 200;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub format: &'static Format,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Opened {
    Paged {
        file: FileInfo,
        engine: &'static str,
        #[serde(flatten)]
        result: OpenPagedResult,
    },
    Text {
        file: FileInfo,
        doc: u32,
        first: Lines,
        language: String,
    },
    Html {
        file: FileInfo,
        html: String,
    },
    Email {
        file: FileInfo,
        doc: u32,
        email: email::Email,
    },
    Image {
        file: FileInfo,
        doc: u32,
    },
    Archive {
        file: FileInfo,
        doc: u32,
        entries: Vec<archive::Entry>,
    },
    Unsupported {
        file: Option<FileInfo>,
        message: String,
    },
}

fn file_info(path: &Path, format: &'static Format) -> FileInfo {
    FileInfo {
        path: path.to_string_lossy().into_owned(),
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        format,
    }
}

fn unsupported(file: Option<FileInfo>, message: impl Into<String>) -> Opened {
    Opened::Unsupported { file, message: message.into() }
}

pub fn image_mime(format_id: &str) -> &'static str {
    match format_id {
        "png" => "image/png",
        "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "heic" => "image/heic",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn language_for(path: &Path, format: &Format) -> String {
    match format.id {
        "code" => path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase(),
        id => id.to_owned(),
    }
}

pub async fn open(reg: &Arc<Registry>, path: PathBuf, password: Option<String>) -> Opened {
    let sniffed = match sniff::sniff(&path) {
        Ok(Some(s)) => s,
        Ok(None) => return unsupported(None, "Lumen doesn't recognize this file type. Try opening it with the app that created it."),
        Err(e) => return unsupported(None, format!("Lumen couldn't read the file: {e}.")),
    };
    let format = sniffed.format;
    let file = file_info(&path, format);
    match format.engine {
        Engine::Mupdf => Opened::Paged { file, engine: "Exact", result: reg.open_paged(path, password).await },
        Engine::Text => {
            let opened = reg.scheduler.run(render::PRIO_VISIBLE, {
                let path = path.clone();
                move || TextDoc::open(&path).map(|d| (d.lines(0, FIRST_LINES), d))
            });
            match opened.await {
                Ok((first, doc)) => {
                    let language = language_for(&path, format);
                    Opened::Text { file, doc: reg.insert(Doc::Text(doc)), first, language }
                }
                Err(e) => unsupported(Some(file), format!("Lumen couldn't read the file: {e}.")),
            }
        }
        Engine::Markdown | Engine::Html => {
            if file.size > INLINE_HTML_LIMIT {
                return unsupported(Some(file), "This file is larger than 64 MB. Open it with your browser instead.");
            }
            let markdown = format.engine == Engine::Markdown;
            let html = reg
                .scheduler
                .run(render::PRIO_VISIBLE, move || {
                    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                    let text = String::from_utf8_lossy(&bytes);
                    Ok::<_, String>(if markdown { markup::markdown(&text) } else { markup::sandboxed(&text) })
                })
                .await;
            match html {
                Ok(html) => Opened::Html { file, html },
                Err(e) => unsupported(Some(file), format!("Lumen couldn't read the file: {e}.")),
            }
        }
        Engine::Email => {
            let is_msg = format.id == "msg";
            match reg.scheduler.run(render::PRIO_VISIBLE, move || email::read(&path, is_msg)).await {
                Ok((email, parts)) => Opened::Email { file, doc: reg.insert(Doc::Email(Arc::new(parts))), email },
                Err(e) => unsupported(Some(file), e),
            }
        }
        Engine::Image => Opened::Image { file, doc: reg.insert(Doc::File(path, image_mime(format.id))) },
        Engine::Archive => {
            let listed = reg.scheduler.run(render::PRIO_VISIBLE, {
                let path = path.clone();
                move || archive::list(&path)
            });
            match listed.await {
                Ok(entries) => Opened::Archive { file, doc: reg.insert(Doc::Archive(path)), entries },
                Err(e) => unsupported(Some(file), e),
            }
        }
        Engine::Office | Engine::Sheet => unsupported(Some(file), "Support for this format is coming in a later build."),
    }
}

pub fn extract(reg: &Registry, doc: u32, index: usize) -> Result<PathBuf, String> {
    let dir = reg.scratch_dir(doc);
    match reg.get(doc).ok_or("This file is closed.")? {
        Doc::Archive(path) => archive::extract(&path, index, &dir),
        Doc::Email(parts) => {
            let part = parts.get(index).ok_or("That attachment no longer exists.")?;
            let name = Path::new(&part.name).file_name().ok_or("The attachment has no usable name.")?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let target = dir.join(name);
            std::fs::write(&target, &part.data).map_err(|e| e.to_string())?;
            Ok(target)
        }
        _ => Err("This file has nothing to extract.".into()),
    }
}
