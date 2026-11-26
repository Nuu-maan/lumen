# Design

Product: a desktop viewer people open many times a day to read a file someone sent them, check it, and close it. Speed and the page itself matter; the chrome should vanish.
Theme: dark chrome, light pages. Dark recedes around the document so the page reads as paper on a desk; pages stay white because that is how the author saw them. Optional "Invert pages" for night reading, off by default.
Accent: amber `oklch(74% 0.13 65)` (#e0912f, the icon's highlight). Neutral: warm gray tinted toward amber (hue 65, chroma 0.004–0.01). Why: one warm hue carries "light/lumen" without the stock indigo look; used only for focus, selection, the active tab, search hits and the primary button.
Type: the system UI font (Segoe UI Variable, SF Pro, Cantarell/Ubuntu via `system-ui`). Why: zero bytes to ship or load, and it matches each OS, which suits a desktop utility. `ui-monospace` for code and logs. Weights 400 and 500 only, 600 for the empty-state heading.
Density: compact. Why: a tool used all day; 13px body, 32px controls, 36px tab strip, 24px status bar.
Screens:
- Viewer → tab strip + toolbar on top, optional left sidebar (thumbnails / outline / sheets / slides), the document canvas, status bar.
- Empty (no tabs) → one focused drop zone with "Open file" and recent files.
- Presenter → full-screen slide, nothing else.
Radius: controls 6px, menus/dialogs 12px, pages 2px.
Depth: hairline borders in the chrome; pages get the one soft shadow (`0 1px 2px` + `0 8px 24px`, warm-black, low opacity). Floating layers (find bar, dialogs) get border + shadow.
Spacing: 4px scale (4, 8, 12, 16, 24, 32, 48).
Motion: transform and opacity only, 150–250 ms, `cubic-bezier(0.23, 1, 0.32, 1)` ease-out. Animated: tab open/close, sidebar slide, find bar and dialogs entering, fast-preview → exact-view crossfade. Not animated: anything a keystroke triggers (page jumps, zoom steps, tab switching), scrolling, tiles. Reduced motion keeps only short opacity fades.
Terms: "file" for what's on disk, "page" for a page, "tab" for an open file. "Exact" (MS Office), "High fidelity" (LibreOffice), "Fast preview" for the engine badge.
Signature: the engine badge in the status bar. It always says which renderer drew the page, and it swaps in place when the exact layout arrives.
