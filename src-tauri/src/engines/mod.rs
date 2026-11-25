pub mod archive;
pub mod email;
pub mod highlight;
pub mod markup;
pub mod mupdf;
pub mod text;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct PageSize {
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Serialize)]
pub struct OutlineItem {
    pub title: String,
    pub page: Option<u32>,
    pub children: Vec<OutlineItem>,
}

#[derive(Debug, Serialize)]
pub struct TextLine {
    pub text: String,
    pub rect: Rect,
}

#[derive(Debug, Serialize)]
pub struct Link {
    pub rect: Rect,
    pub page: Option<u32>,
    pub uri: Option<String>,
}

pub struct Bgr {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub pixels: Vec<u8>,
}

#[derive(Debug)]
pub enum OpenError {
    NeedsPassword,
    WrongPassword,
    Failed(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            OpenError::NeedsPassword => f.write_str("This document is password protected."),
            OpenError::WrongPassword => f.write_str("That password didn't work."),
            OpenError::Failed(msg) => f.write_str(msg),
        }
    }
}

pub trait PagedEngine: Sized {
    fn open(path: &std::path::Path, password: Option<&str>) -> Result<Self, OpenError>;
    fn page_count(&self) -> usize;
    fn page_size(&self, page: usize) -> PageSize;
    fn render_tile(&self, page: usize, scale: f32, tile: Rect) -> Result<Bgr, String>;
    fn text(&self, page: usize) -> Vec<TextLine>;
    fn links(&self, page: usize) -> Vec<Link>;
    fn outline(&self) -> Vec<OutlineItem>;
    fn search(&self, page: usize, needle: &str) -> Vec<Rect>;
}
