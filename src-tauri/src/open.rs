use crate::docs::{Doc, OpenPagedResult, Registry};
use crate::engines::text::{Lines, TextDoc};
use crate::engines::office::{self, ConvertError, Converter};
use crate::engines::{archive, email, fast, markup};
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
        #[serde(skip_serializing_if = "Option::is_none")]
        notes: Option<Vec<String>>,
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
    Office {
        file: FileInfo,
        fast: Option<fast::Fast>,
        encrypted: bool,
        converter: Converter,
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

pub fn pick_converter(prefer_ms_office: bool) -> Converter {
    if prefer_ms_office && office::ms_office_installed() {
        Converter::MsOffice
    } else {
        Converter::LibreOffice
    }
}

fn converted_path(reg: &Registry, key: &str) -> PathBuf {
    reg.cache_root().join("converted").join(format!("{key}.pdf"))
}

pub async fn open(reg: &Arc<Registry>, path: PathBuf, password: Option<String>, prefer_ms_office: bool) -> Opened {
    let sniffed = match sniff::sniff(&path) {
        Ok(Some(s)) => s,
        Ok(None) => return unsupported(None, "Lumen doesn't recognize this file type. Try opening it with the app that created it."),
        Err(e) => return unsupported(None, format!("Lumen couldn't read the file: {e}.")),
    };
    let format = sniffed.format;
    let file = file_info(&path, format);
    match format.engine {
        Engine::Mupdf => Opened::Paged { file, engine: "Exact", notes: None, result: reg.open_paged(path, password).await },
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
        Engine::Office => {
            let converter = pick_converter(prefer_ms_office);
            let id = format.id;
            let encrypted = sniffed.encrypted;
            let (key, fast) = reg
                .scheduler
                .run(render::PRIO_VISIBLE, {
                    let path = path.clone();
                    let cache = reg.cache_root().join("converted");
                    move || {
                        let key = office::content_key(&path, converter).ok();
                        let cached = key.as_ref().is_some_and(|k| cache.join(format!("{k}.pdf")).exists());
                        let fast = match id {
                            _ if encrypted => None,
                            "docx" if cached => None,
                            "docx" => fast::docx(&path),
                            "pptx" => fast::pptx(&path),
                            _ => None,
                        };
                        (key, fast)
                    }
                })
                .await;
            if let Some(cached) = key.map(|k| converted_path(reg, &k)).filter(|p| p.exists()) {
                let notes = fast.map(|f| f.notes).filter(|n| !n.is_empty());
                return Opened::Paged { file, engine: converter.label(), notes, result: reg.open_paged(cached, None).await };
            }
            Opened::Office { file, fast, encrypted, converter }
        }
        Engine::Sheet => unsupported(Some(file), "Support for this format is coming in a later build."),
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

const CONVERT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub async fn exact(reg: &Arc<Registry>, path: PathBuf, password: Option<String>, converter: Converter) -> Result<OpenPagedResult, String> {
    let office = reg.office().clone();
    let out_dir = reg.cache_root().join("converted");
    let result = tauri::async_runtime::spawn_blocking(move || {
        let key = office::content_key(&path, converter).map_err(|e| ConvertError::Failed(e.to_string()))?;
        std::fs::create_dir_all(&out_dir).map_err(|e| ConvertError::Failed(e.to_string()))?;
        let target = out_dir.join(format!("{key}.pdf"));
        let partial = out_dir.join(format!("{key}.{}.partial.pdf", std::process::id()));
        let converted = match (converter, password.as_deref()) {
            (Converter::MsOffice, None) => office.convert_ms_office(&path, &partial, CONVERT_TIMEOUT),
            (_, password) => office.convert_libreoffice(&path, &partial, password, CONVERT_TIMEOUT),
        };
        converted.and_then(|_| std::fs::rename(&partial, &target).map_err(|e| ConvertError::Failed(e.to_string())))?;
        office::trim_cache(&out_dir);
        Ok(target)
    })
    .await
    .map_err(|e| e.to_string())?;
    match result {
        Ok(pdf) => Ok(reg.open_paged(pdf, None).await),
        Err(ConvertError::NeedsPassword) => Ok(OpenPagedResult::NeedsPassword),
        Err(ConvertError::NotInstalled) => Err("not-installed".into()),
        Err(e) => Err(e.to_string()),
    }
}
