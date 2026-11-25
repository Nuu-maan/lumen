pub mod docs;
pub mod engines;
pub mod formats;
pub mod render;
pub mod sniff;

use docs::{OpenPagedResult, Registry};
use engines::{Link, OutlineItem, PagedEngine, Rect, TextLine};
use formats::{Engine, Format};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use tauri::http::{header, Response, StatusCode};
use tauri::ipc::Channel;
use tauri::{Emitter, Manager, State};

struct Launch(Instant);

type Reg<'a> = State<'a, Arc<Registry>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileInfo {
    path: String,
    name: String,
    size: u64,
    format: &'static Format,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Opened {
    Paged {
        file: FileInfo,
        engine: &'static str,
        #[serde(flatten)]
        result: OpenPagedResult,
    },
    Unsupported {
        file: Option<FileInfo>,
        message: String,
    },
}

fn file_info(path: &std::path::Path, format: &'static Format) -> FileInfo {
    FileInfo {
        path: path.to_string_lossy().into_owned(),
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        format,
    }
}

#[tauri::command]
async fn open_document(reg: Reg<'_>, path: String, password: Option<String>) -> Result<Opened, String> {
    let path = PathBuf::from(path);
    let sniffed = match sniff::sniff(&path) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return Ok(Opened::Unsupported { file: None, message: "Lumen doesn't recognize this file type.".into() })
        }
        Err(e) => return Ok(Opened::Unsupported { file: None, message: format!("Couldn't read the file: {e}") }),
    };
    let file = file_info(&path, sniffed.format);
    match sniffed.format.engine {
        Engine::Mupdf => Ok(Opened::Paged { file, engine: "Exact", result: reg.open_paged(path, password).await }),
        _ => Ok(Opened::Unsupported { file: Some(file), message: "Support for this format is coming in a later build.".into() }),
    }
}

#[tauri::command]
fn close_document(reg: Reg<'_>, doc: u32) {
    reg.close(doc);
}

#[tauri::command]
fn set_viewport(reg: Reg<'_>, doc: u32, first: usize, last: usize, scale: f32) {
    if let Some(d) = reg.paged(doc) {
        *d.viewport.lock() = docs::Viewport { first, last, scale };
    }
}

#[tauri::command]
async fn page_text(reg: Reg<'_>, doc: u32, page: usize) -> Result<Vec<TextLine>, String> {
    let d = reg.paged(doc).ok_or("document is closed")?;
    Ok(reg.scheduler.run(render::PRIO_TILE, move || d.engine.text(page)).await)
}

#[tauri::command]
async fn page_links(reg: Reg<'_>, doc: u32, page: usize) -> Result<Vec<Link>, String> {
    let d = reg.paged(doc).ok_or("document is closed")?;
    Ok(reg.scheduler.run(render::PRIO_NEAR, move || d.engine.links(page)).await)
}

#[tauri::command]
async fn outline(reg: Reg<'_>, doc: u32) -> Result<Vec<OutlineItem>, String> {
    let d = reg.paged(doc).ok_or("document is closed")?;
    Ok(reg.scheduler.run(render::PRIO_NEAR, move || d.engine.outline()).await)
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum SearchEvent {
    Hits { page: usize, rects: Vec<Rect> },
    Done,
}

#[tauri::command]
fn search(reg: Reg<'_>, doc: u32, needle: String, from_page: usize, on_event: Channel<SearchEvent>) -> Result<(), String> {
    let d = reg.paged(doc).ok_or("document is closed")?;
    let gen = d.search_gen.fetch_add(1, Ordering::SeqCst) + 1;
    if needle.trim().is_empty() {
        return Ok(());
    }
    let count = d.engine.page_count();
    let remaining = Arc::new(std::sync::atomic::AtomicUsize::new(count));
    for i in 0..count {
        let page = (from_page + i) % count;
        let (d, needle, ch, remaining) = (d.clone(), needle.clone(), on_event.clone(), remaining.clone());
        reg.scheduler.submit(render::PRIO_BACKGROUND, move || {
            if d.search_gen.load(Ordering::SeqCst) != gen {
                return;
            }
            let rects = d.engine.search(page, &needle);
            if !rects.is_empty() {
                let _ = ch.send(SearchEvent::Hits { page, rects });
            }
            if remaining.fetch_sub(1, Ordering::SeqCst) == 1 {
                let _ = ch.send(SearchEvent::Done);
            }
        });
    }
    Ok(())
}

#[tauri::command]
fn app_ready(app: tauri::AppHandle, launch: State<Launch>) -> Vec<String> {
    if std::env::var_os("LUMEN_BENCH_STARTUP").is_some() {
        println!("ready_ms={}", launch.0.elapsed().as_millis());
        app.exit(0);
    }
    std::env::args().skip(1).filter(|a| !a.starts_with('-')).collect()
}

fn respond(status: StatusCode, content_type: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .unwrap()
}

fn handle_protocol(reg: &Arc<Registry>, path: &str, responder: tauri::UriSchemeResponder) {
    let not_found = || respond(StatusCode::NOT_FOUND, "text/plain", Vec::new());
    let Some((id, request)) = docs::parse_tile_path(path) else { return responder.respond(not_found()) };
    let Some(doc) = reg.paged(id) else { return responder.respond(not_found()) };
    let Some((prio, job)) = docs::render_request(&doc, request, None) else {
        return responder.respond(respond(StatusCode::NO_CONTENT, "text/plain", Vec::new()));
    };
    reg.scheduler.submit(prio, move || {
        responder.respond(match job() {
            Ok(bmp) => respond(StatusCode::OK, "image/bmp", bmp),
            Err(e) if e == "cancelled" => respond(StatusCode::NO_CONTENT, "text/plain", Vec::new()),
            Err(e) => respond(StatusCode::INTERNAL_SERVER_ERROR, "text/plain", e.into_bytes()),
        })
    });
}

pub fn run() {
    let launch = Launch(Instant::now());
    let registry = Arc::new(Registry::new());
    let protocol_registry = registry.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let paths: Vec<String> = argv.into_iter().skip(1).filter(|a| !a.starts_with('-')).collect();
            let _ = app.emit("open-paths", paths);
            if let Some(w) = app.get_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol("lumen", move |_ctx, request, responder| {
            handle_protocol(&protocol_registry, request.uri().path(), responder)
        })
        .setup(|app| {
            let window = tauri::window::WindowBuilder::new(app, "main")
                .title("Lumen")
                .inner_size(1200.0, 820.0)
                .min_inner_size(480.0, 360.0)
                .background_color(tauri::window::Color(0x16, 0x18, 0x1d, 0xff))
                .build()?;
            if std::env::var_os("LUMEN_BENCH_STARTUP").is_some() {
                println!("window_ms={}", app.state::<Launch>().0.elapsed().as_millis());
            }
            let size = window.inner_size()?.to_logical::<f64>(window.scale_factor()?);
            window.add_child(
                tauri::webview::WebviewBuilder::new("main", tauri::WebviewUrl::default()).auto_resize(),
                tauri::LogicalPosition::new(0.0, 0.0),
                size,
            )?;
            Ok(())
        })
        .manage(launch)
        .manage(registry)
        .invoke_handler(tauri::generate_handler![
            app_ready,
            open_document,
            close_document,
            set_viewport,
            page_text,
            page_links,
            outline,
            search
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Lumen");
}
