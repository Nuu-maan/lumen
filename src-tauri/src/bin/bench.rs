use lumen_lib::engines::mupdf::MupdfDoc;
use lumen_lib::engines::{PagedEngine, Rect};
use lumen_lib::render::{encode_bmp, Scheduler, PRIO_TILE, PRIO_VISIBLE};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::Instant;

const VIEW_W: f32 = 1200.0;
const VIEW_H: f32 = 800.0;
const DPR: f32 = 1.5;
const TILE: f32 = 512.0;

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bench/samples")
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn pdf_first_page(sched: &Arc<Scheduler>, path: &Path) -> (f64, f64) {
    let t = Instant::now();
    let doc = Arc::new(MupdfDoc::open(path, None).expect("open sample"));
    let size = doc.page_size(0);
    let base_scale = 240.0 / size.w;
    let base = doc.render_tile(0, base_scale, Rect { x: 0.0, y: 0.0, w: 240.0, h: (size.h * base_scale).ceil() }).unwrap();
    encode_bmp(base.width, base.height, base.stride, &base.pixels);
    let base_ms = ms(t);
    let scale = (VIEW_W - 300.0) / size.w * DPR;
    let (pw, ph) = (size.w * scale, (VIEW_H * DPR).min(size.h * scale));
    let (tx, rx) = mpsc::channel();
    let mut n = 0;
    let mut y = 0.0;
    while y < ph {
        let mut x = 0.0;
        while x < pw {
            let (doc, tx) = (doc.clone(), tx.clone());
            let rect = Rect { x, y, w: TILE.min(pw - x), h: TILE.min(ph - y) };
            sched.submit(if n == 0 { PRIO_VISIBLE } else { PRIO_TILE }, move || {
                let t = doc.render_tile(0, scale, rect).unwrap();
                tx.send(encode_bmp(t.width, t.height, t.stride, &t.pixels).len()).unwrap();
            });
            n += 1;
            x += TILE;
        }
        y += TILE;
    }
    for _ in 0..n {
        rx.recv().unwrap();
    }
    (base_ms, ms(t))
}

fn report(name: &str, value: f64, budget: f64, unit: &str) {
    let verdict = if value <= budget { "ok" } else { "OVER" };
    println!("{name:<40} {value:>9.1} {unit:<3} budget {budget:>7.0} {unit:<3} {verdict}");
}

fn main() {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 6);
    let sched = Scheduler::new(threads);
    let (base, sharp) = pdf_first_page(&sched, &samples().join("text-100p.pdf"));
    report("pdf_first_page_preview_ms (100 pages)", base, 150.0, "ms");
    report("pdf_first_page_sharp_ms (100 pages)", sharp, 150.0, "ms");
    let (base, sharp) = pdf_first_page(&sched, &samples().join("images-100mb.pdf"));
    report("pdf_first_page_preview_ms (137 MB)", base, 400.0, "ms");
    report("pdf_first_page_sharp_ms (137 MB)", sharp, 400.0, "ms");
}
