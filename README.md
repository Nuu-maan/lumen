# Lumen

A fast, lightweight desktop viewer for almost any document: PDF, Word, Excel, PowerPoint, e-books, email, text, code and images. Lumen shows files the way their author made them and tells you which engine drew what you're looking at. It's a viewer only, with no editing.

## Install

Download an installer from [Releases](https://github.com/Nuu-maan/lumen/releases):

| Platform | Package |
|---|---|
| Windows 10/11 | `Lumen_x.y.z_x64-setup.exe` (NSIS, per-user) or `.msi` |
| macOS 11+ | `Lumen_x.y.z_universal.dmg` |
| Linux | `.AppImage` or `.deb` |

The Windows installer is about 7.5 MB. Lumen uses the system webview (WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux).

Office formats (DOCX, XLSX, PPTX and friends) render their exact layout through **LibreOffice**, which isn't bundled. Install it from [libreoffice.org](https://www.libreoffice.org/download/download-libreoffice/) and Lumen finds it automatically. Without it, Word and PowerPoint files still open as a text preview, and spreadsheets open in the grid. On Windows with **Microsoft Office** installed, Lumen can render through Office itself instead (status bar: *Render with MS Office*).

## Supported formats

| Engine | Formats |
|---|---|
| MuPDF | PDF, PDF/A, XPS/OXPS, EPUB, MOBI/AZW3, FB2, CBZ, TIFF (multi-page) |
| LibreOffice → PDF → MuPDF | DOCX/DOCM/DOTX, DOC, RTF, ODT, Pages, WPD, HWP, XLSX/XLSM/XLSB, XLS, ODS, Numbers, PPTX/PPSX, PPT, ODP, Keynote, VSDX/VSD, PUB |
| Spreadsheet grid (calamine) | XLSX/XLSM/XLSB, XLS, ODS, CSV, TSV |
| Text | TXT, LOG, JSON, XML, YAML, and source code with syntax highlighting (syntect) |
| Sandboxed web view | Markdown (pulldown-cmark), HTML, MHT |
| Email | EML, MSG, with attachments that open in a new tab |
| Native image decoding | PNG, JPEG, GIF, WebP, AVIF, BMP, ICO, SVG (HEIC on macOS) |
| Archive | ZIP: browse and open any file inside |

The format is detected from the file's contents first and its extension second. Every format is one row in [`src-tauri/src/formats.rs`](src-tauri/src/formats.rs). Not yet supported: CBR, DjVu, HEIC on Windows, PSD and RAW previews ([#8](https://github.com/Nuu-maan/lumen/issues/8)).

## What the status bar badge means

| Badge | Meaning |
|---|---|
| **Exact** | Rendered from the file itself (PDF, e-books, text, images, email). |
| **Exact (MS Office)** | Converted by your installed Microsoft Office: what the author saw. |
| **High fidelity (LibreOffice)** | Converted by LibreOffice, with near-Office layout for tables, fields, sections and charts. |
| **Fast preview** | Text extracted instantly while the exact layout renders. It's replaced in place as soon as the exact view is ready. |
| **Fast preview (grid)** | Spreadsheet cell values. Switch to *Print layout* for the exact pages. |

Converted PDFs are cached by content hash, so reopening an Office file is instant.

## Keyboard

| Action | Shortcut |
|---|---|
| Open file | Ctrl+O |
| Close tab / next tab | Ctrl+W / Ctrl+Tab (Shift for previous) |
| Find, next and previous match | Ctrl+F, Enter or F3, Shift+Enter or Shift+F3 |
| Zoom in / out / actual size | Ctrl+= / Ctrl+- / Ctrl+0, or Ctrl+wheel and pinch |
| Fit width / fit page | Ctrl+1 / Ctrl+2 |
| Rotate | Ctrl+R |
| First / last page | Home / End |
| Previous / next page (single-page mode) | PgUp / PgDn, ← / → |
| Sidebar | Ctrl+B |
| Print / Export as PDF | Ctrl+P / Ctrl+Shift+S |
| Present slides | F5. In presenter mode, arrows, Space and PgDn advance and Esc exits |

On macOS, use Cmd in place of Ctrl.

## Security

Documents are treated as untrusted. PDF JavaScript is compiled out of MuPDF. Office documents load hidden and read-only in a separate LibreOffice process, with macros disabled, link updates off and a 60-second timeout per conversion. HTML, Markdown and email bodies render in a `sandbox=""` iframe with a `default-src 'none'` Content-Security-Policy, so scripts don't run and nothing loads from the network. External links ask before opening. ZIP extraction rejects path traversal.

## Performance

`cargo run --release --bin bench` measures every budget against the real app. The numbers below are from a Windows 11 laptop with a release build:

| Metric | Result | Budget |
|---|---|---|
| Installer (NSIS) | 7.5 MB | 25 MB |
| Window visible | 41 ms | 300 ms |
| PDF first page sharp, 100 pages / 137 MB | 72 ms / 87 ms | 150 ms / 400 ms |
| DOCX fast view | 36 ms | 300 ms |
| DOCX exact view, warm worker, 20 pages | 705 ms | 1.5 s |
| XLSX 100k rows, grid | 392 ms | 1 s |
| 500 MB log, first screen | 14 ms | 200 ms |
| Office file reopen (cache hit) | 57 ms | 200 ms |
| Idle RAM, 100-page PDF | 201 MB | 120 MB, over budget ([#3](https://github.com/Nuu-maan/lumen/issues/3)) |

Lumen's own process uses about 11 MB; WebView2 accounts for the rest. CI runs the bench on every PR and fails on a regression of more than 20% against `main`. Fidelity checks are described in [FIDELITY.md](FIDELITY.md).

## Building from source

Requirements: Rust (stable), Node 22+ with pnpm, LLVM/libclang (MuPDF's bindings are generated with bindgen), and on Linux `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev`. LibreOffice is optional, for Office formats.

```sh
pnpm install
pnpm tauri dev                     # run with hot reload
pnpm tauri build                   # installers in src-tauri/target/release/bundle/
cargo test --manifest-path src-tauri/Cargo.toml
python bench/gen_samples.py        # benchmark inputs (about 640 MB)
cargo run --release --manifest-path src-tauri/Cargo.toml --bin bench
cargo run --release --manifest-path src-tauri/Cargo.toml --bin fidelity
```

On Windows, set `LIBCLANG_PATH` to your LLVM `bin` folder if bindgen can't find it.

## Architecture

```
Svelte UI (webview) ── Tauri commands + lumen:// binary protocol (BMP tiles)
Rust core
  sniff.rs + formats.rs   detect the format, pick an engine
  engines/mupdf.rs        display-list cached, 512 px tiles, text layer, links, outline, search
  engines/office.rs       warm LibreOffice worker (UNO over a pipe) or Office COM, then PDF
  engines/sheet.rs        calamine/csv grid, frozen panes
  engines/text.rs         mmap + background line index + syntect
  render.rs               priority scheduler: visible > tiles > nearby > thumbnails > search,
                          and tiles for pages that scrolled away are dropped
```

## Licenses

Lumen is licensed under **AGPL-3.0-or-later** because it links **MuPDF** (AGPL-3.0, © Artifex). MuPDF was chosen for speed and format coverage (XPS, EPUB, MOBI, FB2, CBZ). A build that can't be AGPL could swap in **PDFium** (`pdfium-render`, BSD-3-Clause/Apache-2.0), at the cost of the non-PDF e-book formats.

| Component | License |
|---|---|
| MuPDF via `mupdf` crate | AGPL-3.0 |
| LibreOffice (separate process, not bundled) | MPL-2.0 |
| Tauri, Svelte, calamine, syntect, pulldown-cmark, mail-parser, zip, quick-xml, blake3 | MIT and/or Apache-2.0 |
| Icons adapted from Lucide | ISC |
