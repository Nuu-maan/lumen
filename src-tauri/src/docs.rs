use crate::engines::mupdf::MupdfDoc;
use crate::engines::{OpenError, PageSize, PagedEngine, Rect};
use crate::render::{self, Scheduler};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Viewport {
    pub first: usize,
    pub last: usize,
    pub scale: f32,
}

pub struct PagedDoc {
    pub engine: MupdfDoc,
    pub viewport: Mutex<Viewport>,
    pub search_gen: AtomicU64,
}

impl PagedDoc {
    pub fn new(engine: MupdfDoc) -> Self {
        PagedDoc { engine, viewport: Mutex::new(Viewport { first: 0, last: 2, scale: 1.0 }), search_gen: AtomicU64::new(0) }
    }

    fn page_priority(&self, page: usize) -> Option<u8> {
        let v = *self.viewport.lock();
        if page >= v.first && page <= v.last {
            Some(render::PRIO_TILE)
        } else if page + 2 >= v.first && page <= v.last + 2 {
            Some(render::PRIO_NEAR)
        } else {
            None
        }
    }
}

pub struct Registry {
    pub scheduler: Arc<Scheduler>,
    paged: RwLock<HashMap<u32, Arc<PagedDoc>>>,
    files: RwLock<HashMap<u32, PathBuf>>,
    next_id: AtomicU32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedInfo {
    pub doc: u32,
    pub pages: Vec<PageSize>,
    pub reflowable: bool,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum OpenPagedResult {
    Ok(PagedInfo),
    NeedsPassword,
    WrongPassword,
    Error { message: String },
}

impl Registry {
    pub fn new() -> Self {
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 6);
        Registry {
            scheduler: Scheduler::new(threads),
            paged: RwLock::new(HashMap::new()),
            files: RwLock::new(HashMap::new()),
            next_id: AtomicU32::new(1),
        }
    }

    pub fn alloc_id(&self) -> u32 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn paged(&self, id: u32) -> Option<Arc<PagedDoc>> {
        self.paged.read().get(&id).cloned()
    }

    pub fn file(&self, id: u32) -> Option<PathBuf> {
        self.files.read().get(&id).cloned()
    }

    pub fn register_file(&self, id: u32, path: PathBuf) {
        self.files.write().insert(id, path);
    }

    pub async fn open_paged(&self, path: PathBuf, password: Option<String>) -> OpenPagedResult {
        let opened = self.scheduler.run(render::PRIO_VISIBLE, move || MupdfDoc::open(&path, password.as_deref())).await;
        match opened {
            Ok(engine) => {
                let id = self.alloc_id();
                let info = PagedInfo {
                    doc: id,
                    pages: (0..engine.page_count()).map(|i| engine.page_size(i)).collect(),
                    reflowable: engine.is_reflowable(),
                };
                self.paged.write().insert(id, Arc::new(PagedDoc::new(engine)));
                OpenPagedResult::Ok(info)
            }
            Err(OpenError::NeedsPassword) => OpenPagedResult::NeedsPassword,
            Err(OpenError::WrongPassword) => OpenPagedResult::WrongPassword,
            Err(OpenError::Failed(message)) => OpenPagedResult::Error { message },
        }
    }

    pub fn close(&self, id: u32) {
        self.paged.write().remove(&id);
        self.files.write().remove(&id);
    }
}

pub enum TileRequest {
    Tile { page: usize, scale: f32, rect: Rect },
    Page { page: usize, width: u32 },
}

pub fn render_request(doc: &Arc<PagedDoc>, request: TileRequest, prio_override: Option<u8>) -> Option<(u8, Box<dyn FnOnce() -> Result<Vec<u8>, String> + Send>)> {
    let doc = doc.clone();
    match request {
        TileRequest::Tile { page, scale, rect } => {
            if page >= doc.engine.page_count() {
                return None;
            }
            let prio = prio_override.or_else(|| doc.page_priority(page))?;
            Some((
                prio,
                Box::new(move || {
                    if doc.page_priority(page).is_none() {
                        return Err("cancelled".into());
                    }
                    let bgr = doc.engine.render_tile(page, scale, rect)?;
                    Ok(render::encode_bmp(bgr.width, bgr.height, bgr.stride, &bgr.pixels))
                }),
            ))
        }
        TileRequest::Page { page, width } => {
            if page >= doc.engine.page_count() {
                return None;
            }
            let size = doc.engine.page_size(page);
            let scale = width as f32 / size.w.max(1.0);
            let rect = Rect { x: 0.0, y: 0.0, w: width as f32, h: (size.h * scale).ceil() };
            let prio = prio_override.unwrap_or(match doc.page_priority(page) {
                Some(render::PRIO_TILE) => render::PRIO_VISIBLE,
                _ => render::PRIO_THUMB,
            });
            Some((
                prio,
                Box::new(move || {
                    let bgr = doc.engine.render_tile(page, scale, rect)?;
                    Ok(render::encode_bmp(bgr.width, bgr.height, bgr.stride, &bgr.pixels))
                }),
            ))
        }
    }
}

pub fn parse_tile_path(path: &str) -> Option<(u32, TileRequest)> {
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let num = |i: usize| parts.get(i).and_then(|s| s.parse::<f32>().ok());
    match parts.first()? {
        &"tile" if parts.len() == 8 => Some((
            parts[1].parse().ok()?,
            TileRequest::Tile {
                page: parts[2].parse().ok()?,
                scale: (num(3)? / 1000.0).clamp(0.01, 40.0),
                rect: Rect { x: num(4)?, y: num(5)?, w: num(6)?.clamp(1.0, 2048.0), h: num(7)?.clamp(1.0, 2048.0) },
            },
        )),
        &"page" if parts.len() == 4 => Some((
            parts[1].parse().ok()?,
            TileRequest::Page { page: parts[2].parse().ok()?, width: (num(3)? as u32).clamp(8, 4096) },
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_paths_parse_and_clamp() {
        match parse_tile_path("/tile/3/7/1500/512/0/512/99999") {
            Some((3, TileRequest::Tile { page: 7, scale, rect })) => {
                assert_eq!(scale, 1.5);
                assert_eq!(rect, Rect { x: 512.0, y: 0.0, w: 512.0, h: 2048.0 });
            }
            _ => panic!("tile path did not parse"),
        }
        assert!(matches!(parse_tile_path("/page/1/0/200"), Some((1, TileRequest::Page { page: 0, width: 200 }))));
        assert!(parse_tile_path("/tile/x/0/1/0/0/1/1").is_none());
        assert!(parse_tile_path("/etc/passwd").is_none());
    }
}
