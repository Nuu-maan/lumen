use super::{Link, OpenError, OutlineItem, PageSize, PagedEngine, Rect, Bgr, TextLine};
use lru::LruCache;
use mupdf::{Colorspace, Device, DisplayList, Document, IRect, Matrix, Pixmap, TextPageFlags};
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::{Arc, OnceLock};

struct SendDoc(Document);

// MuPDF documents are not thread-bound, only non-reentrant: every access goes
// through `MupdfDoc::doc`'s mutex, and display lists are built under it before
// being shared (DisplayList is Send + Sync).
unsafe impl Send for SendDoc {}

type ListCell = Arc<OnceLock<Result<Arc<DisplayList>, String>>>;

pub struct MupdfDoc {
    doc: Mutex<SendDoc>,
    sizes: Vec<PageSize>,
    origins: Vec<(f32, f32)>,
    lists: Mutex<LruCache<usize, ListCell>>,
}

fn err(e: mupdf::Error) -> String {
    e.to_string()
}

fn to_rect(r: mupdf::Rect, origin: (f32, f32)) -> Rect {
    Rect { x: r.x0 - origin.0, y: r.y0 - origin.1, w: r.x1 - r.x0, h: r.y1 - r.y0 }
}

impl MupdfDoc {
    fn display_list(&self, page: usize) -> Result<Arc<DisplayList>, String> {
        let cell = self.lists.lock().get_or_insert(page, Default::default).clone();
        cell.get_or_init(|| {
            let doc = self.doc.lock();
            let page = doc.0.load_page(page as i32).map_err(err)?;
            page.to_display_list(true).map(Arc::new).map_err(err)
        })
        .clone()
    }

    fn with_page<T>(&self, page: usize, f: impl FnOnce(&mupdf::Page) -> Result<T, mupdf::Error>) -> Option<T> {
        let doc = self.doc.lock();
        let page = doc.0.load_page(page as i32).ok()?;
        f(&page).ok()
    }

    pub fn export_pdf(&self, source: &Path, target: &Path) -> Result<(), String> {
        let doc = self.doc.lock();
        if doc.0.is_pdf() {
            return std::fs::copy(source, target).map(|_| ()).map_err(|e| e.to_string());
        }
        let target = target.to_str().ok_or("The target path is not valid Unicode.")?;
        let pdf = doc.0.convert_to_pdf(0, self.sizes.len() as i32 - 1, 0).map_err(err)?;
        pdf.save(target).map_err(err)
    }

    pub fn is_reflowable(&self) -> bool {
        self.doc.lock().0.is_reflowable().unwrap_or(false)
    }
}

impl PagedEngine for MupdfDoc {
    fn open(path: &Path, password: Option<&str>) -> Result<Self, OpenError> {
        let path_str = path.to_str().ok_or_else(|| OpenError::Failed("The file path is not valid Unicode.".into()))?;
        let mut doc = Document::open(path_str).map_err(|e| OpenError::Failed(e.to_string()))?;
        if doc.needs_password().unwrap_or(false) {
            match password {
                None => return Err(OpenError::NeedsPassword),
                Some(p) if !doc.authenticate(p).unwrap_or(false) => return Err(OpenError::WrongPassword),
                Some(_) => {}
            }
        }
        if doc.is_reflowable().unwrap_or(false) {
            doc.layout(450.0, 640.0, 11.0).map_err(|e| OpenError::Failed(e.to_string()))?;
        }
        let count = doc.page_count().map_err(|e| OpenError::Failed(e.to_string()))?.max(0) as usize;
        if count == 0 {
            return Err(OpenError::Failed("The document has no pages.".into()));
        }
        let mut sizes = Vec::with_capacity(count);
        let mut origins = Vec::with_capacity(count);
        for i in 0..count {
            let bounds = doc.load_page(i as i32).and_then(|p| p.bounds()).unwrap_or(mupdf::Rect {
                x0: 0.0,
                y0: 0.0,
                x1: 612.0,
                y1: 792.0,
            });
            sizes.push(PageSize { w: bounds.x1 - bounds.x0, h: bounds.y1 - bounds.y0 });
            origins.push((bounds.x0, bounds.y0));
        }
        Ok(MupdfDoc {
            doc: Mutex::new(SendDoc(doc)),
            sizes,
            origins,
            lists: Mutex::new(LruCache::new(NonZeroUsize::new(24).unwrap())),
        })
    }

    fn page_count(&self) -> usize {
        self.sizes.len()
    }

    fn page_size(&self, page: usize) -> PageSize {
        self.sizes[page]
    }

    fn render_tile(&self, page: usize, scale: f32, tile: Rect) -> Result<Bgr, String> {
        let list = self.display_list(page)?;
        let (ox, oy) = self.origins[page];
        let mut ctm = Matrix::new_scale(scale, scale);
        ctm.pre_translate(-ox, -oy);
        let area = IRect {
            x0: tile.x as i32,
            y0: tile.y as i32,
            x1: (tile.x + tile.w) as i32,
            y1: (tile.y + tile.h) as i32,
        };
        let mut pixmap = Pixmap::new_with_rect(&Colorspace::device_bgr(), area, false).map_err(err)?;
        pixmap.clear_with(255).map_err(err)?;
        {
            let device = Device::from_pixmap_with_clip(&pixmap, area).map_err(err)?;
            list.run(&device, &ctm, area.into()).map_err(err)?;
        }
        Ok(Bgr { width: pixmap.width(), height: pixmap.height(), stride: pixmap.stride() as usize, pixels: pixmap.samples().to_vec() })
    }

    fn text(&self, page: usize) -> Vec<TextLine> {
        let Ok(list) = self.display_list(page) else { return Vec::new() };
        let Ok(text) = list.to_text_page(TextPageFlags::empty()) else { return Vec::new() };
        let origin = self.origins[page];
        let mut lines = Vec::new();
        for block in text.blocks() {
            for line in block.lines() {
                let s: String = line.chars().filter_map(|c| c.char()).collect();
                if !s.trim().is_empty() {
                    lines.push(TextLine { text: s, rect: to_rect(line.bounds(), origin) });
                }
            }
        }
        lines
    }

    fn links(&self, page: usize) -> Vec<Link> {
        let origin = self.origins[page];
        self.with_page(page, |p| {
            Ok(p.links()?
                .map(|l| Link {
                    rect: to_rect(l.bounds, origin),
                    page: l.dest.map(|d| d.loc.page_number),
                    uri: (l.dest.is_none() && !l.uri.is_empty()).then(|| l.uri.clone()),
                })
                .collect())
        })
        .unwrap_or_default()
    }

    fn outline(&self) -> Vec<OutlineItem> {
        fn convert(items: Vec<mupdf::Outline>) -> Vec<OutlineItem> {
            items
                .into_iter()
                .map(|o| OutlineItem {
                    title: o.title,
                    page: o.dest.map(|d| d.loc.page_number),
                    children: convert(o.down),
                })
                .collect()
        }
        self.doc.lock().0.outlines().map(convert).unwrap_or_default()
    }

    fn search(&self, page: usize, needle: &str) -> Vec<Rect> {
        let Ok(list) = self.display_list(page) else { return Vec::new() };
        let origin = self.origins[page];
        list.search(needle, 500)
            .map(|quads| {
                quads
                    .iter()
                    .map(|q| {
                        let x0 = q.ul.x.min(q.ll.x);
                        let y0 = q.ul.y.min(q.ur.y);
                        let x1 = q.ur.x.max(q.lr.x);
                        let y1 = q.ll.y.max(q.lr.y);
                        Rect { x: x0 - origin.0, y: y0 - origin.1, w: x1 - x0, h: y1 - y0 }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
