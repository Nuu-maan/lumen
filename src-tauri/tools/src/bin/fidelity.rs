use lumen_lib::engines::mupdf::MupdfDoc;
use lumen_lib::engines::office::Office;
use lumen_lib::engines::{Bgr, PagedEngine, Rect};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SCALE: f32 = 1.0;
const THRESHOLD: f64 = 2.0;
const CHANNEL_TOLERANCE: i32 = 24;

const COVERS: &[(&str, &str)] = &[
    ("tables.docx", "merged cells, nested table, repeated header rows, borders and shading"),
    ("layout.docx", "two-column section, header/footer fields, footnote, endnote, text box, floating image"),
    ("scripts.docx", "Arabic and Hebrew RTL, CJK, missing font, tracked changes shown as final"),
    ("numbers.xlsx", "currency/percent/date formats, conditional formatting, merged title, print area"),
    ("deck.pptx", "slide master, title and body placeholders, speaker notes"),
    ("scanned.pdf", "image-only scanned page"),
];

fn render(doc: &MupdfDoc, page: usize) -> Bgr {
    let size = doc.page_size(page);
    let rect = Rect { x: 0.0, y: 0.0, w: (size.w * SCALE).ceil(), h: (size.h * SCALE).ceil() };
    doc.render_tile(page, SCALE, rect).expect("render page")
}

fn mismatch(a: &Bgr, b: &Bgr) -> (f64, Vec<u8>) {
    let (w, h) = (a.width.max(b.width) as usize, a.height.max(b.height) as usize);
    let px = |img: &Bgr, x: usize, y: usize| -> [i32; 3] {
        if x >= img.width as usize || y >= img.height as usize {
            return [255, 255, 255];
        }
        let i = y * img.stride + x * 3;
        [img.pixels[i] as i32, img.pixels[i + 1] as i32, img.pixels[i + 2] as i32]
    };
    let mut diff = vec![255u8; w * h * 3];
    let mut bad = 0usize;
    for y in 0..h {
        for x in 0..w {
            let (p, q) = (px(a, x, y), px(b, x, y));
            let i = (y * w + x) * 3;
            if p.iter().zip(q).any(|(c, d)| (c - d).abs() > CHANNEL_TOLERANCE) {
                bad += 1;
                diff[i..i + 3].copy_from_slice(&[0, 0, 220]);
            } else {
                let g = (p.iter().sum::<i32>() / 3 / 3 + 170) as u8;
                diff[i..i + 3].copy_from_slice(&[g, g, g]);
            }
        }
    }
    (bad as f64 * 100.0 / (w * h) as f64, diff)
}

fn write_png(path: &Path, width: usize, height: usize, bgr: &[u8]) {
    let mut raw = Vec::with_capacity((width * 3 + 1) * height);
    for row in bgr.chunks(width * 3) {
        raw.push(0);
        raw.extend(row.chunks(3).flat_map(|p| [p[2], p[1], p[0]]));
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&raw).unwrap();
    let data = z.finish().unwrap();
    let chunk = |out: &mut Vec<u8>, kind: &[u8], body: &[u8]| {
        out.extend((body.len() as u32).to_be_bytes());
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(body);
        out.extend(kind);
        out.extend(body);
        out.extend(crc.sum().to_be_bytes());
    };
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend((width as u32).to_be_bytes());
    ihdr.extend((height as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &data);
    chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png).unwrap();
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let work = std::env::temp_dir().join(format!("lumen-fidelity-{}", std::process::id()));
    let diffs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/fidelity");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&diffs).unwrap();
    let update = std::env::args().any(|a| a == "--update-references");
    let office = Office::new(work.join("office"));
    let mut rows = Vec::new();
    let mut failed = false;
    for (name, covers) in COVERS {
        let input = root.join(name);
        let stem = name.rsplit_once('.').unwrap().0;
        let reference = root.join("reference").join(format!("{stem}.pdf"));
        let test: PathBuf = if name.ends_with(".pdf") {
            input.clone()
        } else {
            let out = work.join(format!("{stem}.pdf"));
            if let Err(e) = office.convert_libreoffice(&input, &out, None, Duration::from_secs(120)) {
                rows.push(format!("| {name} | – | – | conversion failed: {e} | {covers} |"));
                failed = true;
                continue;
            }
            out
        };
        if update && test != reference {
            std::fs::create_dir_all(reference.parent().unwrap()).unwrap();
            std::fs::copy(&test, &reference).expect("update reference");
        }
        let (a, b) = (MupdfDoc::open(&reference, None).expect("open reference"), MupdfDoc::open(&test, None).expect("open output"));
        let pages = a.page_count().max(b.page_count());
        let mut worst: f64 = 0.0;
        for page in 0..pages {
            let ratio = if page >= a.page_count() || page >= b.page_count() {
                100.0
            } else {
                let (ra, rb) = (render(&a, page), render(&b, page));
                let (ratio, diff) = mismatch(&ra, &rb);
                if ratio > THRESHOLD {
                    write_png(&diffs.join(format!("{stem}-p{}.png", page + 1)), ra.width.max(rb.width) as usize, ra.height.max(rb.height) as usize, &diff);
                }
                ratio
            };
            worst = worst.max(ratio);
        }
        let ok = worst <= THRESHOLD && a.page_count() == b.page_count();
        failed |= !ok;
        rows.push(format!(
            "| {name} | {} / {} | {worst:.2}% | {} | {covers} |",
            a.page_count(),
            b.page_count(),
            if ok { "pass" } else { "MISMATCH" }
        ));
        println!("{name:<14} {worst:>6.2}%  {}", if ok { "pass" } else { "MISMATCH" });
    }
    office.shutdown();
    let table = format!(
        "| Fixture | Pages (reference / Lumen) | Worst page mismatch | Result (≤ {THRESHOLD}%) | Covers |\n|---|---|---|---|---|\n{}\n",
        rows.join("\n")
    );
    let doc = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../FIDELITY.md");
    if let Ok(existing) = std::fs::read_to_string(&doc) {
        let (head, rest) = existing.split_once("<!-- results -->").unwrap_or((&existing, ""));
        let tail = rest.split_once("<!-- /results -->").map(|(_, t)| t).unwrap_or("");
        std::fs::write(&doc, format!("{head}<!-- results -->\n{table}<!-- /results -->{tail}")).unwrap();
    }
    let _ = std::fs::remove_dir_all(&work);
    if failed {
        std::process::exit(1);
    }
}
