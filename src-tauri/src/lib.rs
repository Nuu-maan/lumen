pub mod docs;
pub mod engines;
pub mod formats;
pub mod open;
pub mod render;
pub mod sniff;

use docs::{Doc, Registry};
use engines::{Link, OutlineItem, PagedEngine, Rect, TextLine};
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

#[tauri::command]
async fn open_document(reg: Reg<'_>, path: String, password: Option<String>, prefer_ms_office: bool) -> Result<open::Opened, String> {
    Ok(open::open(&reg, PathBuf::from(path), password, prefer_ms_office).await)
}

#[tauri::command]
async fn office_exact(
    reg: Reg<'_>,
    path: String,
    password: Option<String>,
    converter: engines::office::Converter,
) -> Result<docs::OpenPagedResult, String> {
    open::exact(&reg, PathBuf::from(path), password, converter).await
}

#[tauri::command]
fn ms_office_available() -> bool {
    engines::office::ms_office_installed()
}

#[tauri::command]
async fn text_lines(reg: Reg<'_>, doc: u32, start: usize, count: usize) -> Result<engines::text::Lines, String> {
    let d = reg.text(doc).ok_or("document is closed")?;
    Ok(reg.scheduler.run(render::PRIO_VISIBLE, move || d.lines(start, count.min(2000))).await)
}

#[tauri::command]
async fn text_search(reg: Reg<'_>, doc: u32, needle: String, limit: usize) -> Result<Vec<(usize, usize)>, String> {
    let d = reg.text(doc).ok_or("document is closed")?;
    Ok(reg.scheduler.run(render::PRIO_NEAR, move || d.search(&needle, limit)).await)
}

#[tauri::command]
async fn sheet_info(reg: Reg<'_>, doc: u32, sheet: usize) -> Result<engines::sheet::SheetInfo, String> {
    let d = reg.sheet(doc).ok_or("document is closed")?;
    reg.scheduler.run(render::PRIO_VISIBLE, move || d.info(sheet)).await
}

#[tauri::command]
async fn sheet_cells(reg: Reg<'_>, doc: u32, sheet: usize, rows: [usize; 2], cols: [usize; 2]) -> Result<Vec<Vec<String>>, String> {
    let d = reg.sheet(doc).ok_or("document is closed")?;
    let (rows, cols) = (rows[0]..rows[1].min(rows[0] + 2000), cols[0]..cols[1].min(cols[0] + 256));
    reg.scheduler.run(render::PRIO_VISIBLE, move || d.cells(sheet, rows, cols)).await
}

#[tauri::command]
async fn export_pdf(reg: Reg<'_>, doc: u32, target: String) -> Result<(), String> {
    let d = reg.paged(doc).ok_or("document is closed")?;
    reg.scheduler.run(render::PRIO_VISIBLE, move || d.engine.export_pdf(&d.source, std::path::Path::new(&target))).await
}

#[tauri::command]
async fn extract_item(reg: Reg<'_>, doc: u32, index: usize) -> Result<String, String> {
    let reg = reg.inner().clone();
    reg.clone().scheduler.run(render::PRIO_VISIBLE, move || open::extract(&reg, doc, index).map(|p| p.to_string_lossy().into_owned())).await
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

fn path_args(args: impl Iterator<Item = String>, cwd: &std::path::Path) -> Vec<String> {
    args.filter(|a| !a.starts_with('-')).map(|a| cwd.join(a).to_string_lossy().into_owned()).collect()
}

#[tauri::command]
fn app_ready(app: tauri::AppHandle, launch: State<Launch>) -> Vec<String> {
    if std::env::var_os("LUMEN_BENCH_STARTUP").is_some() {
        println!("ready_ms={}", launch.0.elapsed().as_millis());
        app.exit(0);
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    path_args(std::env::args().skip(1), &cwd)
}

#[tauri::command]
fn bench_mark(app: tauri::AppHandle, name: String, ms: f64) {
    let Ok(wanted) = std::env::var("LUMEN_BENCH") else { return };
    println!("{name}_ms={ms:.1}");
    if wanted == name {
        app.exit(0);
    }
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
    if let docs::TileRequest::File = request {
        let Some(Doc::File(file, mime)) = reg.get(id) else { return responder.respond(not_found()) };
        return reg.scheduler.submit(render::PRIO_VISIBLE, move || {
            responder.respond(match std::fs::read(&file) {
                Ok(bytes) => respond(StatusCode::OK, mime, bytes),
                Err(_) => not_found(),
            })
        });
    }
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
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let paths = path_args(argv.into_iter().skip(1), std::path::Path::new(&cwd));
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
            std::thread::spawn(engines::office::ms_office_installed);
            if let Some(dir) = std::env::var_os("LUMEN_CACHE_DIR").map(PathBuf::from).or_else(|| app.path().app_cache_dir().ok()) {
                let _ = app.state::<Arc<Registry>>().cache_dir.set(dir);
            }
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
            bench_mark,
            open_document,
            office_exact,
            ms_office_available,
            close_document,
            text_lines,
            text_search,
            extract_item,
            export_pdf,
            sheet_info,
            sheet_cells,
            set_viewport,
            page_text,
            page_links,
            outline,
            search
        ])
        .build(tauri::generate_context!())
        .expect("failed to start Lumen")
        .run(|app, event| match event {
            tauri::RunEvent::Exit => app.state::<Arc<Registry>>().office().shutdown(),
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Opened { urls } => {
                let paths: Vec<String> = urls.iter().filter_map(|u| u.to_file_path().ok()).map(|p| p.to_string_lossy().into_owned()).collect();
                let _ = app.emit("open-paths", paths);
            }
            _ => {}
        });
}
