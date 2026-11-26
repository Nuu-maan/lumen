use lumen_lib::engines::mupdf::MupdfDoc;
use lumen_lib::engines::office::{find_libreoffice, Office};
use lumen_lib::engines::{PagedEngine, Rect};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const REGRESSION: f64 = 1.2;

struct Metric {
    name: &'static str,
    budget: f64,
    unit: &'static str,
}

const METRICS: &[Metric] = &[
    Metric { name: "installer_mb", budget: 25.0, unit: "MB" },
    Metric { name: "binary_mb", budget: 25.0, unit: "MB" },
    Metric { name: "window_visible_ms", budget: 300.0, unit: "ms" },
    Metric { name: "idle_ram_100p_pdf_mb", budget: 120.0, unit: "MB" },
    Metric { name: "pdf_first_page_ms", budget: 150.0, unit: "ms" },
    Metric { name: "pdf_100mb_first_page_ms", budget: 400.0, unit: "ms" },
    Metric { name: "docx_fast_view_ms", budget: 300.0, unit: "ms" },
    Metric { name: "docx_exact_warm_20p_ms", budget: 1500.0, unit: "ms" },
    Metric { name: "xlsx_100k_grid_ms", budget: 1000.0, unit: "ms" },
    Metric { name: "text_500mb_first_screen_ms", budget: 200.0, unit: "ms" },
    Metric { name: "office_reopen_ms", budget: 200.0, unit: "ms" },
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sample(name: &str) -> PathBuf {
    root().join("bench/samples").join(name)
}

fn app() -> PathBuf {
    std::env::current_exe().unwrap().with_file_name(format!("lumen{}", std::env::consts::EXE_SUFFIX))
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

fn launch(cache: &Path, env: &[(&str, &str)], file: Option<&Path>, key: &str) -> Option<f64> {
    let mut cmd = Command::new(app());
    cmd.env("LUMEN_CACHE_DIR", cache).stdout(Stdio::piped()).stderr(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    if let Some(f) = file {
        cmd.arg(f);
    }
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    let wanted = format!("{key}=");
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(v) = line.strip_prefix(&wanted).and_then(|v| v.parse::<f64>().ok()) {
                let _ = tx.send(v);
            }
        }
    });
    let value = rx.recv_timeout(Duration::from_secs(60)).ok();
    if child.wait_timeout(Duration::from_secs(10)).is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    value
}

trait WaitTimeout {
    fn wait_timeout(&mut self, d: Duration) -> Option<std::process::ExitStatus>;
}

impl WaitTimeout for std::process::Child {
    fn wait_timeout(&mut self, d: Duration) -> Option<std::process::ExitStatus> {
        let start = Instant::now();
        while start.elapsed() < d {
            if let Ok(Some(s)) = self.try_wait() {
                return Some(s);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

fn runs(n: usize, mut f: impl FnMut() -> Option<f64>) -> Option<f64> {
    median((0..n).filter_map(|_| f()).collect())
}

fn calibrate() -> Option<f64> {
    runs(7, || {
        let t = Instant::now();
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut v: Vec<u64> = (0..6_000_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x
            })
            .collect();
        v.sort_unstable();
        std::hint::black_box(&v);
        Some(t.elapsed().as_secs_f64() * 1000.0)
    })
}

fn idle_ram_mb(cache: &Path) -> Option<f64> {
    let mut child = Command::new(app()).env("LUMEN_CACHE_DIR", cache).arg(sample("text-100p.pdf")).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()?;
    std::thread::sleep(Duration::from_secs(6));
    let pid = child.id();
    let out = if cfg!(windows) {
        let script = format!(
            "function T($i){{ $k = Get-CimInstance Win32_Process -Filter \"ParentProcessId=$i\"; @($i) + ($k | ForEach-Object {{ T $_.ProcessId }}) }}; \
             [math]::Round(((Get-Process -Id (T {pid}) -ErrorAction SilentlyContinue | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB), 1)"
        );
        Command::new("powershell").args(["-NoProfile", "-Command", &script]).output().ok()?
    } else {
        Command::new("sh").arg("-c").arg(format!("ps -o rss= -p {pid} $(pgrep -P {pid}) | awk '{{s+=$1}} END {{print s/1024}}'")).output().ok()?
    };
    let _ = child.kill();
    let _ = child.wait();
    String::from_utf8_lossy(&out.stdout).trim().replace(',', ".").parse().ok()
}

fn docx_exact_warm(work: &Path) -> Option<f64> {
    find_libreoffice()?;
    let office = Office::new(work.join("office"));
    office.warm_up();
    let input = sample("report-20p.docx");
    let times: Vec<f64> = (0..3)
        .filter_map(|i| {
            let out = work.join(format!("exact-{i}.pdf"));
            let t = Instant::now();
            office.convert_libreoffice(&input, &out, None, Duration::from_secs(60)).ok()?;
            let doc = MupdfDoc::open(&out, None).ok()?;
            let size = doc.page_size(0);
            doc.render_tile(0, 1.5, Rect { x: 0.0, y: 0.0, w: 512.0, h: 512.0f32.min(size.h * 1.5) }).ok()?;
            Some(t.elapsed().as_secs_f64() * 1000.0)
        })
        .collect();
    office.shutdown();
    median(times)
}

fn installer_mb() -> Option<f64> {
    let bundle = std::env::current_exe().ok()?.parent()?.join("bundle");
    let mut sizes = Vec::new();
    for dir in ["nsis", "msi", "dmg", "appimage", "deb"] {
        for entry in std::fs::read_dir(bundle.join(dir)).into_iter().flatten().flatten() {
            sizes.push(entry.metadata().ok()?.len() as f64 / 1e6);
        }
    }
    sizes.into_iter().reduce(f64::max)
}

fn measure() -> BTreeMap<&'static str, f64> {
    let work = std::env::temp_dir().join(format!("lumen-bench-{}", std::process::id()));
    let fresh = |name: &str| {
        let dir = work.join(name);
        let _ = std::fs::remove_dir_all(&dir);
        dir
    };
    let mut r = BTreeMap::new();
    let mut put = |k: &'static str, v: Option<f64>| {
        if let Some(v) = v {
            r.insert(k, v);
        }
    };
    put("calibration_ms", calibrate());
    put("installer_mb", installer_mb());
    put("binary_mb", std::fs::metadata(app()).ok().map(|m| m.len() as f64 / 1e6));
    let cache = fresh("cache");
    put("window_visible_ms", runs(5, || launch(&cache, &[("LUMEN_BENCH_STARTUP", "1")], None, "window_ms")));
    put("idle_ram_100p_pdf_mb", idle_ram_mb(&cache));
    let sharp = [("LUMEN_BENCH", "first_page_sharp")];
    put("pdf_first_page_ms", runs(3, || launch(&cache, &sharp, Some(&sample("text-100p.pdf")), "first_page_sharp_ms")));
    put("pdf_100mb_first_page_ms", runs(3, || launch(&cache, &sharp, Some(&sample("images-100mb.pdf")), "first_page_sharp_ms")));
    put(
        "docx_fast_view_ms",
        runs(3, || launch(&fresh("docx-fast"), &[("LUMEN_BENCH", "office_fast")], Some(&sample("report-20p.docx")), "office_fast_ms")),
    );
    put("docx_exact_warm_20p_ms", docx_exact_warm(&fresh("exact")));
    put("xlsx_100k_grid_ms", runs(3, || launch(&cache, &[("LUMEN_BENCH", "sheet_grid")], Some(&sample("orders-100k.xlsx")), "sheet_grid_ms")));
    put(
        "text_500mb_first_screen_ms",
        runs(3, || launch(&cache, &[("LUMEN_BENCH", "text_first_screen")], Some(&sample("huge-500mb.log")), "text_first_screen_ms")),
    );
    if find_libreoffice().is_some() {
        let reopen = fresh("reopen");
        launch(&reopen, &sharp, Some(&sample("report-20p.docx")), "first_page_sharp_ms");
        put("office_reopen_ms", runs(3, || launch(&reopen, &sharp, Some(&sample("report-20p.docx")), "first_page_sharp_ms")));
    }
    let _ = std::fs::remove_dir_all(&work);
    r
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    if !sample("text-100p.pdf").exists() {
        eprintln!("bench samples missing: run `python bench/gen_samples.py` first");
        std::process::exit(2);
    }
    let results = measure();
    let baseline: BTreeMap<String, f64> = arg("--check").and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let speed = match (results.get("calibration_ms"), baseline.get("calibration_ms")) {
        (Some(&now), Some(&then)) => {
            println!("runner speed vs baseline: {:.2}x slower (baseline times scaled by this)", now / then);
            Some(now / then)
        }
        _ if baseline.is_empty() => None,
        _ => {
            println!("baseline has no runner calibration: skipping the regression check this run");
            None
        }
    };
    let mut regressions = Vec::new();
    println!("{:<30} {:>10} {:>10}  {:<6} {}", "metric", "value", "budget", "status", "vs baseline");
    for m in METRICS {
        let Some(&v) = results.get(m.name) else {
            println!("{:<30} {:>10} {:>7} {:<2}  {:<6}", m.name, "skipped", m.budget, m.unit, "-");
            continue;
        };
        let within = if v <= m.budget { "ok" } else { "OVER" };
        let (scale, floor) = if m.unit == "ms" { (speed.unwrap_or(1.0), 15.0) } else { (1.0, 5.0) };
        let trend = match baseline.get(m.name).filter(|_| speed.is_some()).map(|b| b * scale) {
            Some(b) if v > b * REGRESSION && v - b > floor => {
                regressions.push(format!("{}: {v:.1} vs baseline {b:.1}", m.name));
                format!("REGRESSED from {b:.1}")
            }
            Some(b) => format!("{:+.0}%", (v / b - 1.0) * 100.0),
            None => String::new(),
        };
        println!("{:<30} {:>10.1} {:>7} {:<2}  {:<6} {}", m.name, v, m.budget, m.unit, within, trend);
    }
    if let Some(out) = arg("--out") {
        std::fs::write(out, serde_json::to_string_pretty(&results).unwrap()).expect("write results");
    }
    if !regressions.is_empty() {
        eprintln!("regressed more than 20%:\n  {}", regressions.join("\n  "));
        std::process::exit(1);
    }
}
