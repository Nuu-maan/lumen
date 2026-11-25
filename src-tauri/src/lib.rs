mod formats;
mod sniff;

use std::time::Instant;
use tauri::{Emitter, Manager};

struct Launch(Instant);

#[tauri::command]
fn app_ready(app: tauri::AppHandle, launch: tauri::State<Launch>) {
    if std::env::var_os("LUMEN_BENCH_STARTUP").is_some() {
        println!("ready_ms={}", launch.0.elapsed().as_millis());
        app.exit(0);
    }
}

pub fn run() {
    let launch = Launch(Instant::now());
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let _ = app.emit("open-paths", argv.into_iter().skip(1).collect::<Vec<_>>());
            if let Some(w) = app.get_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
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
        .invoke_handler(tauri::generate_handler![app_ready])
        .run(tauri::generate_context!())
        .expect("failed to start Lumen");
}
