use parking_lot::Mutex;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const WORKER_SCRIPT: &str = include_str!("office_worker.py");
const COM_SCRIPT: &str = include_str!("office_com.ps1");
const START_TIMEOUT: Duration = Duration::from_secs(90);
const IDLE_SHUTDOWN: Duration = Duration::from_secs(10 * 60);
const CACHE_LIMIT: u64 = 2 << 30;

#[derive(Debug, PartialEq)]
pub enum ConvertError {
    NotInstalled,
    NeedsPassword,
    Timeout,
    Failed(String),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ConvertError::NotInstalled => f.write_str("LibreOffice isn't installed, so only the fast preview is available."),
            ConvertError::NeedsPassword => f.write_str("This document is password protected."),
            ConvertError::Timeout => f.write_str("Rendering the exact layout took longer than 60 seconds and was stopped."),
            ConvertError::Failed(m) => write!(f, "The exact layout couldn't be rendered: {m}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Converter {
    LibreOffice,
    MsOffice,
}

impl Converter {
    pub fn label(self) -> &'static str {
        match self {
            Converter::LibreOffice => "High fidelity (LibreOffice)",
            Converter::MsOffice => "Exact (MS Office)",
        }
    }
}

pub struct Install {
    pub soffice: PathBuf,
    pub python: PathBuf,
}

pub fn find_libreoffice() -> Option<Install> {
    let candidates: Vec<PathBuf> = if cfg!(windows) {
        ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
            .iter()
            .filter_map(|v| std::env::var_os(v))
            .map(|base| PathBuf::from(base).join("LibreOffice").join("program"))
            .collect()
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Applications/LibreOffice.app/Contents/MacOS")]
    } else {
        vec![PathBuf::from("/usr/lib/libreoffice/program"), PathBuf::from("/opt/libreoffice/program"), PathBuf::from("/usr/lib64/libreoffice/program")]
    };
    candidates.into_iter().find_map(|dir| {
        let soffice = dir.join(if cfg!(windows) { "soffice.exe" } else { "soffice" });
        let python = if cfg!(windows) {
            dir.join("python.exe")
        } else if cfg!(target_os = "macos") {
            dir.join("../Resources/python")
        } else {
            PathBuf::from("/usr/bin/python3")
        };
        soffice.exists().then_some(Install { soffice, python })
    })
}

pub fn ms_office_installed() -> bool {
    static INSTALLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *INSTALLED.get_or_init(|| {
        cfg!(windows)
            && Command::new("reg")
            .args(["query", r"HKCR\Word.Application\CurVer"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

struct Worker {
    soffice: Child,
    python: Child,
    stdin: ChildStdin,
    replies: Receiver<String>,
}

impl Worker {
    fn kill(self) {
        let Worker { mut soffice, mut python, stdin, .. } = self;
        drop(stdin);
        let deadline = Instant::now() + Duration::from_secs(3);
        while matches!(python.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = python.kill();
        let _ = python.wait();
        kill_tree(&mut soffice);
        let _ = soffice.wait();
    }
}

#[cfg(unix)]
fn kill_tree(child: &mut Child) {
    let _ = Command::new("kill").args(["-KILL", &format!("-{}", child.id())]).stderr(Stdio::null()).status();
    let _ = child.kill();
}

#[cfg(windows)]
fn kill_tree(child: &mut Child) {
    let _ = hide_window(Command::new("taskkill").args(["/T", "/F", "/PID", &child.id().to_string()]))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
}

pub struct Office {
    work_dir: PathBuf,
    worker: Mutex<Option<Worker>>,
    last_used: Mutex<Instant>,
    worker_broken: AtomicBool,
}

fn soffice_command(install: &Install, profile: &Path) -> Command {
    let mut cmd = Command::new(&install.soffice);
    cmd.args(["--headless", "--invisible", "--nologo", "--norestore", "--nodefault", "--nolockcheck", "--nofirststartwizard"])
        .arg(format!("-env:UserInstallation={}", url_from_path(profile)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    hide_window(&mut cmd);
    cmd
}

#[cfg(windows)]
fn hide_window(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000)
}

#[cfg(not(windows))]
fn hide_window(cmd: &mut Command) -> &mut Command {
    cmd
}

impl Office {
    pub fn new(work_dir: PathBuf) -> std::sync::Arc<Self> {
        let office = std::sync::Arc::new(Office {
            work_dir,
            worker: Mutex::new(None),
            last_used: Mutex::new(Instant::now()),
            worker_broken: AtomicBool::new(cfg!(target_os = "macos")),
        });
        let weak = std::sync::Arc::downgrade(&office);
        std::thread::Builder::new()
            .name("office-reaper".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_secs(30));
                let Some(office) = weak.upgrade() else { return };
                if office.last_used.lock().elapsed() > IDLE_SHUTDOWN {
                    if let Some(w) = office.worker.lock().take() {
                        w.kill();
                    }
                }
            })
            .expect("spawn office reaper");
        office
    }

    pub fn shutdown(&self) {
        if let Some(w) = self.worker.lock().take() {
            w.kill();
        }
    }

    fn start(&self, install: &Install) -> Result<Worker, ConvertError> {
        let fail = |e: std::io::Error| ConvertError::Failed(format!("couldn't start LibreOffice ({e})"));
        std::fs::create_dir_all(&self.work_dir).map_err(fail)?;
        let script = self.work_dir.join("lumen_office_worker.py");
        std::fs::write(&script, WORKER_SCRIPT).map_err(fail)?;
        let pipe = format!("lumen_{}", std::process::id());
        let mut soffice = soffice_command(install, &self.work_dir.join("lo-profile"))
            .arg(format!("--accept=pipe,name={pipe};urp;"))
            .spawn()
            .map_err(fail)?;
        let mut python = match hide_window(
            Command::new(&install.python).arg(&script).arg(&pipe).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()),
        )
        .spawn()
        {
            Ok(python) => python,
            Err(e) => {
                kill_tree(&mut soffice);
                let _ = soffice.wait();
                return Err(fail(e));
            }
        };
        let stdin = python.stdin.take().expect("piped stdin");
        let stdout = python.stdout.take().expect("piped stdout");
        let (tx, replies) = mpsc::channel();
        std::thread::Builder::new()
            .name("office-replies".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        return;
                    }
                }
            })
            .map_err(fail)?;
        let worker = Worker { soffice, python, stdin, replies };
        match worker.replies.recv_timeout(START_TIMEOUT) {
            Ok(line) if line.contains("\"ready\"") => Ok(worker),
            Ok(line) => {
                worker.kill();
                Err(ConvertError::Failed(line))
            }
            Err(_) => {
                worker.kill();
                Err(ConvertError::Failed("LibreOffice didn't start".into()))
            }
        }
    }

    pub fn warm_up(&self) {
        if self.worker_broken.load(Ordering::Relaxed) {
            return;
        }
        if let Some(install) = find_libreoffice() {
            let mut guard = self.worker.lock();
            if guard.is_none() {
                *guard = self.start(&install).ok();
            }
        }
    }

    pub fn convert_libreoffice(&self, input: &Path, output: &Path, password: Option<&str>, timeout: Duration) -> Result<(), ConvertError> {
        self.convert_libreoffice_pages(input, output, password, timeout, None)
    }

    /// Like `convert_libreoffice`, but spreadsheets export only `pages` (a LibreOffice page range like "1-100").
    pub fn convert_libreoffice_pages(
        &self,
        input: &Path,
        output: &Path,
        password: Option<&str>,
        timeout: Duration,
        pages: Option<&str>,
    ) -> Result<(), ConvertError> {
        let install = find_libreoffice().ok_or(ConvertError::NotInstalled)?;
        *self.last_used.lock() = Instant::now();
        let mut guard = self.worker.lock();
        for attempt in 0..2 {
            if guard.is_none() && password.is_none() && self.worker_broken.load(Ordering::Relaxed) {
                return self.convert_cli(&install, input, output, timeout, pages);
            }
            if guard.is_none() {
                match self.start(&install) {
                    Ok(worker) => *guard = Some(worker),
                    Err(_) if password.is_none() => {
                        self.worker_broken.store(true, Ordering::Relaxed);
                        return self.convert_cli(&install, input, output, timeout, pages);
                    }
                    Err(e) => return Err(e),
                }
            }
            let worker = guard.as_mut().unwrap();
            let request = serde_json::json!({ "in": input, "out": output, "password": password, "pages": pages }).to_string();
            if writeln!(worker.stdin, "{request}").and_then(|_| worker.stdin.flush()).is_err() {
                guard.take().unwrap().kill();
                continue;
            }
            match worker.replies.recv_timeout(timeout) {
                Ok(line) if line.contains("\"ok\"") => return Ok(()),
                Ok(line) => {
                    let message = serde_json::from_str::<serde_json::Value>(&line)
                        .ok()
                        .and_then(|v| v["error"].as_str().map(str::to_owned))
                        .unwrap_or(line);
                    return Err(classify_failure(&message, password.is_some()));
                }
                Err(RecvTimeoutError::Timeout) => {
                    guard.take().unwrap().kill();
                    return Err(ConvertError::Timeout);
                }
                Err(RecvTimeoutError::Disconnected) => {
                    guard.take().unwrap().kill();
                    if attempt == 1 {
                        return Err(ConvertError::Failed("LibreOffice stopped unexpectedly".into()));
                    }
                }
            }
        }
        Err(ConvertError::Failed("LibreOffice stopped unexpectedly".into()))
    }

    fn convert_cli(&self, install: &Install, input: &Path, output: &Path, timeout: Duration, pages: Option<&str>) -> Result<(), ConvertError> {
        // Spreadsheet page ranges go through the PDF filter's options, which LibreOffice reads as JSON.
        let format = match pages {
            Some(range) => format!("pdf:calc_pdf_Export:{}", serde_json::json!({ "PageRange": { "type": "string", "value": range } })),
            None => "pdf".to_owned(),
        };
        let out_dir = output.with_extension("lo-out");
        let _ = std::fs::remove_dir_all(&out_dir);
        std::fs::create_dir_all(&out_dir).map_err(|e| ConvertError::Failed(e.to_string()))?;
        let mut child = soffice_command(install, &self.work_dir.join("lo-profile-cli"))
            .args(["--convert-to", &format, "--outdir"])
            .arg(&out_dir)
            .arg(input)
            .spawn()
            .map_err(|e| ConvertError::Failed(format!("couldn't start LibreOffice ({e})")))?;
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| ConvertError::Failed(e.to_string()))? {
                break status;
            }
            if started.elapsed() > timeout {
                kill_tree(&mut child);
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&out_dir);
                return Err(ConvertError::Timeout);
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let produced = std::fs::read_dir(&out_dir)
            .ok()
            .and_then(|d| d.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "pdf")));
        let moved = produced.map(|p| std::fs::rename(p, output));
        let _ = std::fs::remove_dir_all(&out_dir);
        match moved {
            Some(Ok(())) => Ok(()),
            Some(Err(e)) => Err(ConvertError::Failed(e.to_string())),
            None => Err(ConvertError::Failed(format!("LibreOffice produced no PDF ({status})"))),
        }
    }

    pub fn convert_ms_office(&self, input: &Path, output: &Path, timeout: Duration) -> Result<(), ConvertError> {
        let script = self.work_dir.join("lumen_office_com.ps1");
        std::fs::create_dir_all(&self.work_dir).and_then(|_| std::fs::write(&script, COM_SCRIPT)).map_err(|e| ConvertError::Failed(e.to_string()))?;
        let mut child = hide_window(
            Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(&script)
                .arg(input)
                .arg(output)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped()),
        )
        .spawn()
        .map_err(|e| ConvertError::Failed(e.to_string()))?;
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().map_err(|e| ConvertError::Failed(e.to_string()))? {
                if status.success() && output.exists() {
                    return Ok(());
                }
                let mut err = String::new();
                let _ = child.stderr.take().map(|mut e| e.read_to_string(&mut err));
                return Err(classify_failure(err.trim(), false));
            }
            if started.elapsed() > timeout {
                let _ = child.kill();
                return Err(ConvertError::Timeout);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn classify_failure(message: &str, had_password: bool) -> ConvertError {
    let lower = message.to_ascii_lowercase();
    if !had_password && (lower.contains("password") || lower.contains("load failed") || lower.contains("encrypt")) {
        ConvertError::NeedsPassword
    } else {
        ConvertError::Failed(message.to_owned())
    }
}

fn url_from_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let encoded: String = s.chars().map(|c| if c == ' ' { "%20".to_owned() } else { c.to_string() }).collect();
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

pub fn content_key(path: &Path, converter: Converter) -> std::io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_mmap(path)?;
    Ok(format!("{}-{}", hasher.finalize().to_hex(), if converter == Converter::MsOffice { "mso" } else { "lo" }))
}

pub fn trim_cache(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    for (_, len, path) in files {
        if total <= CACHE_LIMIT {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_are_absolute_and_escape_spaces() {
        assert_eq!(url_from_path(Path::new(r"C:\Users\A B\cache")), "file:///C:/Users/A%20B/cache");
        assert_eq!(url_from_path(Path::new("/home/a b")), "file:///home/a%20b");
    }

    #[test]
    fn missing_password_is_detected_only_without_one() {
        assert_eq!(classify_failure("IOException: load failed", false), ConvertError::NeedsPassword);
        assert!(matches!(classify_failure("IOException: load failed", true), ConvertError::Failed(_)));
    }

    #[test]
    fn same_content_gets_same_key() {
        let dir = std::env::temp_dir().join(format!("lumen-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.docx"), dir.join("b.docx"));
        std::fs::write(&a, b"same").unwrap();
        std::fs::write(&b, b"same").unwrap();
        assert_eq!(content_key(&a, Converter::LibreOffice).unwrap(), content_key(&b, Converter::LibreOffice).unwrap());
        assert_ne!(content_key(&a, Converter::LibreOffice).unwrap(), content_key(&a, Converter::MsOffice).unwrap());
    }

    #[test]
    #[ignore = "needs LibreOffice installed"]
    fn converts_a_document_with_libreoffice() {
        let dir = std::env::temp_dir().join(format!("lumen-lo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("in.rtf");
        std::fs::write(&input, br"{\rtf1\ansi Hello {\b world}.}").unwrap();
        let office = Office::new(dir.join("work"));
        let out = dir.join("out.pdf");
        office.convert_libreoffice(&input, &out, None, Duration::from_secs(120)).unwrap();
        assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
        office.shutdown();
        let cli_out = dir.join("in.v2.pdf");
        office.convert_cli(&find_libreoffice().unwrap(), &input, &cli_out, Duration::from_secs(120), None).unwrap();
        assert!(std::fs::read(&cli_out).unwrap().starts_with(b"%PDF"));
        let sheet = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/numbers.xlsx");
        let ranged = dir.join("sheet.pdf");
        office.convert_cli(&find_libreoffice().unwrap(), &sheet, &ranged, Duration::from_secs(120), Some("1-1")).unwrap();
        assert!(std::fs::read(&ranged).unwrap().starts_with(b"%PDF"));
    }
}
