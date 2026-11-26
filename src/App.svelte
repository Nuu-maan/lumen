<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { printImage, printPaged } from "./lib/print";
  import { openPath as systemOpen, openUrl } from "@tauri-apps/plugin-opener";
  import { api, converterLabel } from "./lib/api";
  import { store, activeTab, openPath, load, closeTab, cycleTab, forgetRecent, openExtracted, retryOffice } from "./lib/tabs.svelte";
  import { formatSize } from "./lib/format";
  import { PagedState, type Fit } from "./lib/paged.svelte";
  import SheetView from "./views/SheetView.svelte";
  import Icon from "./components/Icon.svelte";
  import Dialog from "./components/Dialog.svelte";
  import FindBar from "./components/FindBar.svelte";
  import PagedView from "./views/PagedView.svelte";
  import Sidebar from "./views/Sidebar.svelte";
  import TextView from "./views/TextView.svelte";
  import HtmlView from "./views/HtmlView.svelte";
  import EmailView from "./views/EmailView.svelte";
  import ImageView from "./views/ImageView.svelte";
  import ArchiveView from "./views/ArchiveView.svelte";

  let sidebarOpen = $state(localStorage.getItem("lumen.sidebar") !== "0");
  let findOpen = $state(false);
  let dragging = $state(false);
  let password = $state("");
  let pendingLink = $state<string | null>(null);
  let pageInput = $state("");
  let pagedView: PagedView | undefined = $state();
  let presenting = $state(false);
  let notesOpen = $state(localStorage.getItem("lumen.notes") !== "0");
  let msOffice = $state(false);
  let notice = $state<string | null>(null);

  const tab = $derived(activeTab());
  const sheet = $derived(tab?.view.kind === "sheet" ? tab.view.state : null);
  const paged = $derived(
    tab?.view.kind === "paged" ? tab.view.state : sheet?.mode === "print" && sheet.print instanceof PagedState ? sheet.print : null,
  );
  const image = $derived(tab?.view.kind === "image" ? tab.view.state : null);
  const textFind = $derived(tab?.view.kind === "text" ? tab.view.find : null);
  const notes = $derived(tab?.view.kind === "paged" && tab.view.notes?.some((n) => n.trim()) ? tab.view.notes : null);
  const isDeck = $derived(tab?.file?.format.family === "presentation" && !!paged);
  const officeEngine = $derived(tab?.view.kind === "paged" && tab.file?.format.engine === "office");

  $effect(() => localStorage.setItem("lumen.notes", notesOpen ? "1" : "0"));

  function flash(message: string) {
    notice = message;
    setTimeout(() => notice === message && (notice = null), 4000);
  }

  async function print() {
    if (paged) await printPaged(paged.doc, paged.pages);
    else if (image?.natural && tab?.doc != null) await printImage(tab.doc, image.natural);
    else if (tab) flash("Printing works for documents, spreadsheets in print layout, and images.");
  }

  async function exportPdf() {
    if (!paged || !tab) return flash("Export as PDF works for documents and spreadsheets in print layout.");
    const target = await saveDialog({
      title: "Export as PDF",
      defaultPath: tab.name.replace(/\.[^.]+$/, "") + ".pdf",
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!target) return;
    try {
      await api.exportPdf(paged.doc, target);
      flash(`Saved ${target.split(/[\\/]/).pop()}`);
    } catch (e) {
      flash(`Couldn't export the PDF: ${e}`);
    }
  }

  let savedView: { fit: Fit; zoom: number } | null = null;
  async function present(on: boolean) {
    if (!paged || on === presenting) return;
    if (on) {
      savedView = { fit: paged.fit, zoom: paged.zoom };
      paged.fit = "page";
    } else if (savedView) {
      paged.fit = savedView.fit;
      paged.zoom = savedView.zoom;
    }
    presenting = on;
    await getCurrentWindow().setFullscreen(on).catch(() => {});
  }

  function switchConverter() {
    if (!tab) return;
    const useMs = localStorage.getItem("lumen.msoffice") === "0";
    localStorage.setItem("lumen.msoffice", useMs ? "1" : "0");
    void load(tab.id);
  }

  $effect(() => localStorage.setItem("lumen.sidebar", sidebarOpen ? "1" : "0"));
  $effect(() => {
    pageInput = paged ? String(paged.page + 1) : "";
  });
  $effect(() => {
    void store.active;
    findOpen = false;
  });
  $effect(() => {
    document.title = tab ? `${tab.name} - Lumen` : "Lumen";
  });

  async function chooseFiles() {
    const picked = await openDialog({ multiple: true, title: "Open file" });
    for (const p of picked ?? []) await openPath(p);
  }

  function openAll(paths: string[]) {
    for (const p of paths) void openPath(p);
  }

  function zoom(direction: 1 | -1) {
    if (image) return image.zoomStep(direction);
    if (!paged) return;
    const before = paged.zoom;
    paged.zoomStep(direction);
    const after = paged.zoom;
    paged.zoom = before;
    pagedView?.zoomTo(after);
  }

  function submitPage(e: Event) {
    e.preventDefault();
    const n = parseInt(pageInput, 10);
    if (paged && !Number.isNaN(n)) paged.goTo(n - 1);
    else if (paged) pageInput = String(paged.page + 1);
  }

  function onkeydown(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    const typing = (e.target as HTMLElement)?.closest?.("input, textarea");
    const key = e.key.toLowerCase();
    if (mod && key === "o") return e.preventDefault(), void chooseFiles();
    if (mod && key === "w") return e.preventDefault(), tab && closeTab(tab.id);
    if (mod && (e.key === "Tab" || e.code === "Tab")) return e.preventDefault(), cycleTab(e.shiftKey ? -1 : 1);
    if (e.ctrlKey && (e.key === "PageUp" || e.key === "PageDown")) return e.preventDefault(), cycleTab(e.key === "PageUp" ? -1 : 1);
    if (mod && key === "b") return e.preventDefault(), (sidebarOpen = !sidebarOpen);
    if (mod && key === "p") return e.preventDefault(), void print();
    if (mod && e.shiftKey && key === "s") return e.preventDefault(), void exportPdf();
    if (presenting && paged) {
      e.preventDefault();
      if (e.key === "Escape" || e.key === "F5") return void present(false);
      if (["ArrowRight", "ArrowDown", "PageDown", " ", "Enter", "n"].includes(e.key)) return paged.goTo(paged.page + 1);
      if (["ArrowLeft", "ArrowUp", "PageUp", "Backspace", "p"].includes(e.key)) return paged.goTo(paged.page - 1);
      if (e.key === "Home") return paged.goTo(0);
      if (e.key === "End") return paged.goTo(paged.pageCount - 1);
      return;
    }
    if (e.key === "F5" && paged) return e.preventDefault(), void present(true);
    if (textFind && mod && key === "f") return e.preventDefault(), (findOpen = true);
    if (textFind && e.key === "F3") return e.preventDefault(), textFind.step(e.shiftKey ? -1 : 1);
    if (image) {
      if (mod && (e.key === "=" || e.key === "+")) return e.preventDefault(), image.zoomStep(1);
      if (mod && e.key === "-") return e.preventDefault(), image.zoomStep(-1);
      if (mod && e.key === "0") return e.preventDefault(), image.setZoom(1);
      if (mod && e.key === "1") return e.preventDefault(), (image.fit = "width");
      if (mod && e.key === "2") return e.preventDefault(), (image.fit = "page");
      if (mod && key === "r") return e.preventDefault(), image.rotate();
    }
    if (!paged) return;
    if (mod && key === "f") return e.preventDefault(), (findOpen = true);
    if (mod && (e.key === "=" || e.key === "+")) return e.preventDefault(), zoom(1);
    if (mod && e.key === "-") return e.preventDefault(), zoom(-1);
    if (mod && e.key === "0") return e.preventDefault(), pagedView?.zoomTo(1);
    if (mod && e.key === "1") return e.preventDefault(), (paged.fit = "width");
    if (mod && e.key === "2") return e.preventDefault(), (paged.fit = "page");
    if (mod && key === "r") return e.preventDefault(), paged.rotate();
    if (e.key === "F3") return e.preventDefault(), paged.selectHit(paged.hitIndex + (e.shiftKey ? -1 : 1));
    if (typing) return;
    const single = paged.mode === "single";
    if (e.key === "Home") return e.preventDefault(), paged.goTo(0);
    if (e.key === "End") return e.preventDefault(), paged.goTo(paged.pageCount - 1);
    if (single && (e.key === "PageDown" || e.key === "ArrowRight" || e.key === " "))
      return e.preventDefault(), paged.goTo(paged.page + 1);
    if (single && (e.key === "PageUp" || e.key === "ArrowLeft")) return e.preventDefault(), paged.goTo(paged.page - 1);
  }

  onMount(() => {
    const forwarded = listen<string[]>("open-paths", (e) => openAll(e.payload));
    const unlisten = [
      forwarded,
      getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type === "enter" || e.payload.type === "over") dragging = true;
        else if (e.payload.type === "leave") dragging = false;
        else if (e.payload.type === "drop") {
          dragging = false;
          openAll(e.payload.paths);
        }
      }),
    ];
    requestAnimationFrame(() => requestAnimationFrame(() => forwarded.then(api.ready).then(openAll)));
    void api.msOfficeAvailable().then((v) => (msOffice = v));
    return () => unlisten.forEach((u) => u.then((f) => f()));
  });

  const zoomLabel = $derived(paged ? `${Math.round(paged.zoom * 100)}%` : "");
</script>

<svelte:window {onkeydown} />

<div class="app">
  {#if store.tabs.length}
    <header class="tabs" role="tablist" aria-label="Open files">
      {#each store.tabs as t (t.id)}
        <div class="tab" class:active={t.id === store.active} role="tab" aria-selected={t.id === store.active}>
          <button class="tab-label" title={t.path} onclick={() => (store.active = t.id)} onauxclick={(e) => e.button === 1 && closeTab(t.id)}>
            {t.name}
          </button>
          <button class="tab-close btn" aria-label="Close {t.name}" title="Close tab (Ctrl+W)" onclick={() => closeTab(t.id)}>
            <Icon name="close" size={14} />
          </button>
        </div>
      {/each}
      <button class="btn new-tab" title="Open file (Ctrl+O)" aria-label="Open file" onclick={chooseFiles}><Icon name="open" /></button>
    </header>

    {#if paged || image || textFind || sheet?.converter}
    <div class="toolbar" role="toolbar" aria-label="View controls">
      {#if sheet?.converter && tab}
        {@const t = tab}
        <div class="segmented" role="radiogroup" aria-label="Spreadsheet view">
          <button class="btn" role="radio" aria-checked={sheet.mode === "grid"} onclick={() => (sheet.mode = "grid")}>
            <Icon name="grid" /> Grid
          </button>
          <button class="btn" role="radio" aria-checked={sheet.mode === "print"} onclick={() => sheet.showPrint(t.path)}>
            <Icon name="print" /> Print layout
          </button>
        </div>
        {#if paged}<span class="sep"></span>{/if}
      {/if}
      {#if paged}
        <button class="btn" aria-pressed={sidebarOpen} title="Sidebar (Ctrl+B)" aria-label="Toggle sidebar" onclick={() => (sidebarOpen = !sidebarOpen)}>
          <Icon name="sidebar" />
        </button>
        <span class="sep"></span>
        <form class="pager" onsubmit={submitPage}>
          <input class="field page-input" bind:value={pageInput} aria-label="Page number" onfocus={(e) => e.currentTarget.select()} />
          <span class="muted">of {paged.pageCount}</span>
        </form>
        <span class="sep"></span>
        <button class="btn" title="Zoom out (Ctrl+-)" aria-label="Zoom out" onclick={() => zoom(-1)}><Icon name="zoomOut" /></button>
        <button class="btn zoom-label" title="Actual size (Ctrl+0)" onclick={() => pagedView?.zoomTo(1)}>{zoomLabel}</button>
        <button class="btn" title="Zoom in (Ctrl+=)" aria-label="Zoom in" onclick={() => zoom(1)}><Icon name="zoomIn" /></button>
        <button class="btn" aria-pressed={paged.fit === "width"} title="Fit width (Ctrl+1)" aria-label="Fit width" onclick={() => (paged.fit = "width")}>
          <Icon name="fitWidth" />
        </button>
        <button class="btn" aria-pressed={paged.fit === "page"} title="Fit page (Ctrl+2)" aria-label="Fit page" onclick={() => (paged.fit = "page")}>
          <Icon name="fitPage" />
        </button>
        <span class="sep"></span>
        <button class="btn" title="Rotate (Ctrl+R)" aria-label="Rotate clockwise" onclick={() => paged.rotate()}><Icon name="rotate" /></button>
        <button
          class="btn"
          aria-pressed={paged.mode === "single"}
          title={paged.mode === "single" ? "Continuous scroll" : "Single page"}
          aria-label="Toggle single page"
          onclick={() => (paged.mode = paged.mode === "single" ? "scroll" : "single")}
        >
          <Icon name={paged.mode === "single" ? "single" : "scroll"} />
        </button>
        <button class="btn" aria-pressed={paged.invert} title="Invert pages for night reading" aria-label="Invert pages" onclick={() => (paged.invert = !paged.invert)}>
          <Icon name="moon" />
        </button>
        <span class="grow"></span>
        {#if notes}
          <button class="btn" aria-pressed={notesOpen} title="Speaker notes" onclick={() => (notesOpen = !notesOpen)}><Icon name="notes" /> Notes</button>
        {/if}
        {#if isDeck}
          <button class="btn" title="Present full screen (F5)" onclick={() => present(true)}><Icon name="present" /> Present</button>
        {/if}
        <button class="btn" title="Print (Ctrl+P)" aria-label="Print" onclick={print}><Icon name="print" /></button>
        <button class="btn" title="Export as PDF (Ctrl+Shift+S)" aria-label="Export as PDF" onclick={exportPdf}><Icon name="download" /></button>
        <button class="btn" title="Find (Ctrl+F)" aria-label="Find" onclick={() => (findOpen = true)}><Icon name="search" /></button>
      {:else if image}
        <button class="btn" title="Zoom out (Ctrl+-)" aria-label="Zoom out" onclick={() => image.zoomStep(-1)}><Icon name="zoomOut" /></button>
        <button class="btn zoom-label" title="Actual size (Ctrl+0)" onclick={() => image.setZoom(1)}>{Math.round(image.zoom * 100)}%</button>
        <button class="btn" title="Zoom in (Ctrl+=)" aria-label="Zoom in" onclick={() => image.zoomStep(1)}><Icon name="zoomIn" /></button>
        <button class="btn" aria-pressed={image.fit === "page"} title="Fit to window (Ctrl+2)" aria-label="Fit to window" onclick={() => (image.fit = "page")}>
          <Icon name="fitPage" />
        </button>
        <span class="sep"></span>
        <button class="btn" title="Rotate (Ctrl+R)" aria-label="Rotate clockwise" onclick={() => image.rotate()}><Icon name="rotate" /></button>
        <span class="grow"></span>
        <button class="btn" title="Print (Ctrl+P)" aria-label="Print" onclick={print}><Icon name="print" /></button>
      {:else if textFind}
        <span class="grow"></span>
        <button class="btn" title="Find (Ctrl+F)" aria-label="Find" onclick={() => (findOpen = true)}><Icon name="search" /></button>
      {/if}
    </div>
    {/if}

    <main class="main">
      {#if paged && sidebarOpen}<Sidebar state={paged} />{/if}
      <section class="stage">
        {#if !tab}
          <div></div>
        {:else if tab.view.kind === "loading"}
          <div class="skeleton-stage" aria-busy="true" aria-label="Opening {tab.name}">
            <div class="skeleton-page"><span></span><span></span><span></span><span></span><span></span></div>
          </div>
        {:else if tab.view.kind === "paged"}
          {#key tab.id}
            <div class="swap" class:swapped={tab.view.swapped} class:with-notes={notes && notesOpen}>
              {#if !presenting}
                <PagedView bind:this={pagedView} state={tab.view.state} onExternalLink={(uri) => (pendingLink = uri)} />
              {/if}
            </div>
            {#if notes && notesOpen}
              <aside class="notes" aria-label="Speaker notes">
                <h2>Notes for slide {tab.view.state.page + 1}</h2>
                <p>{notes[tab.view.state.page]?.trim() || "No notes for this slide."}</p>
              </aside>
            {/if}
          {/key}
          {#if findOpen}
            <FindBar
              query={tab.view.state.query}
              count={tab.view.state.hits.length}
              index={tab.view.state.hitIndex}
              searching={tab.view.state.searching}
              onsearch={(q) => paged?.search(q)}
              onnext={(d) => paged?.selectHit(paged.hitIndex + d)}
              onclose={() => {
                findOpen = false;
                paged?.search("");
              }}
            />
          {/if}
        {:else if tab.view.kind === "text"}
          {@const view = tab.view}
          {#key tab.id}<TextView doc={view.doc} first={view.first} find={view.find} />{/key}
          {#if findOpen}
            <FindBar
              query={view.find.query}
              count={view.find.hits.length}
              index={view.find.index}
              searching={view.find.searching}
              more={view.find.capped}
              onsearch={(q) => view.find.search(q)}
              onnext={(d) => view.find.step(d)}
              onclose={() => {
                findOpen = false;
                void view.find.search("");
              }}
            />
          {/if}
        {:else if tab.view.kind === "office"}
          {@const view = tab.view}
          {@const t = tab}
          {#if view.status === "failed" || (view.status === "notInstalled" && !view.fast)}
            <div class="center-card" role="alert">
              <Icon name="alert" size={20} />
              {#if view.status === "notInstalled"}
                <h1>Install LibreOffice to view {t.name}</h1>
                <p class="muted">Lumen renders Office files with LibreOffice (free). Install it, then reopen this file.</p>
              {:else}
                <h1>Couldn't render {t.name}</h1>
                <p class="muted">{view.message}</p>
              {/if}
              <div class="row">
                {#if view.status === "notInstalled"}
                  <button class="btn secondary" onclick={() => openUrl("https://www.libreoffice.org/download/download-libreoffice/")}>Download LibreOffice</button>
                {:else}
                  <button class="btn secondary" onclick={() => load(t.id)}>Retry</button>
                {/if}
                <button class="btn primary" onclick={() => systemOpen(t.path)}>Open with system app</button>
              </div>
            </div>
          {:else if view.fast}
            {#if view.status === "notInstalled"}
              <div class="banner" role="status">
                Showing a text preview. Install LibreOffice for the exact layout.
                <button class="btn" onclick={() => openUrl("https://www.libreoffice.org/download/download-libreoffice/")}>Download LibreOffice</button>
              </div>
            {/if}
            <div class="fast" class:with-banner={view.status === "notInstalled"}><HtmlView html={view.fast.html} title="{t.name} preview" /></div>
          {:else}
            <div class="skeleton-stage" aria-busy="true" aria-label="Rendering {t.name}">
              <div class="skeleton-page"><span></span><span></span><span></span><span></span><span></span></div>
            </div>
          {/if}
        {:else if tab.view.kind === "sheet"}
          {@const st = tab.view.state}
          {@const t = tab}
          {#if st.mode === "grid"}
            {#if st.infos[st.sheet]}
              {#key `${tab.id}/${st.sheet}`}
                <SheetView doc={st.doc} names={st.names} info={st.infos[st.sheet]} sheet={st.sheet} onselect={(i) => st.select(i)} />
              {/key}
            {/if}
          {:else if st.print instanceof PagedState}
            {@const ps = st.print}
            {#key ps.doc}
              <div class="swap swapped"><PagedView bind:this={pagedView} state={ps} onExternalLink={(uri) => (pendingLink = uri)} /></div>
            {/key}
            {#if findOpen}
              <FindBar
                query={ps.query}
                count={ps.hits.length}
                index={ps.hitIndex}
                searching={ps.searching}
                onsearch={(q) => ps.search(q)}
                onnext={(d) => ps.selectHit(ps.hitIndex + d)}
                onclose={() => {
                  findOpen = false;
                  ps.search("");
                }}
              />
            {/if}
          {:else if st.print && typeof st.print === "object"}
            <div class="center-card" role="alert">
              <Icon name="alert" size={20} />
              <h1>Couldn't render the print layout</h1>
              <p class="muted">{st.print.error}</p>
              <div class="row">
                <button class="btn secondary" onclick={() => (st.mode = "grid")}>Back to grid</button>
                <button class="btn primary" onclick={() => systemOpen(t.path)}>Open with system app</button>
              </div>
            </div>
          {:else}
            <div class="skeleton-stage" aria-busy="true" aria-label="Rendering print layout">
              <div class="skeleton-page"><span></span><span></span><span></span><span></span><span></span></div>
            </div>
          {/if}
        {:else if tab.view.kind === "html"}
          <HtmlView html={tab.view.html} title={tab.name} />
        {:else if tab.view.kind === "email"}
          {@const view = tab.view}
          <EmailView email={view.email} onopen={(i) => openExtracted(view.doc, i)} />
        {:else if tab.view.kind === "image"}
          {@const view = tab.view}
          {#if view.state.failed}
            {@const t = tab}
            <div class="center-card" role="alert">
              <Icon name="alert" size={20} />
              <h1>Couldn't display {t.name}</h1>
              <p class="muted">This system's web view can't decode {t.file?.format.label ?? "this image"}. Open it with the system app instead.</p>
              <div class="row">
                <button class="btn primary" onclick={() => systemOpen(t.path)}>Open with system app</button>
              </div>
            </div>
          {:else}
            {#key tab.id}<ImageView doc={view.doc} state={view.state} name={tab.name} />{/key}
          {/if}
        {:else if tab.view.kind === "archive"}
          {@const view = tab.view}
          <ArchiveView entries={view.entries} onopen={(i) => openExtracted(view.doc, i)} />
        {:else if tab.view.kind === "password"}
          {@const t = tab}
          <form
            class="center-card"
            onsubmit={(e) => {
              e.preventDefault();
              void retryOffice(t.id, password).then(() => (password = ""));
            }}
          >
            <Icon name="lock" size={20} />
            <h1>{t.name} is password protected</h1>
            <p class="muted">Enter the password to open it. Lumen doesn't store it.</p>
            <input class="field" type="password" bind:value={password} aria-label="Password" autocomplete="off" />
            {#if tab.view.wrong}<p class="error" role="alert">That password didn't work. Check caps lock and try again.</p>{/if}
            <div class="row">
              <button class="btn secondary" type="button" onclick={() => closeTab(t.id)}>Close tab</button>
              <button class="btn primary" type="submit" disabled={!password}>Open file</button>
            </div>
          </form>
        {:else if tab.view.kind === "error"}
          {@const t = tab}
          <div class="center-card" role="alert">
            <Icon name="alert" size={20} />
            <h1>Couldn't open {t.name}</h1>
            <p class="muted">{tab.view.message}</p>
            <div class="row">
              <button class="btn secondary" onclick={() => load(t.id)}>Retry</button>
              <button class="btn primary" onclick={() => systemOpen(t.path)}>Open with system app</button>
            </div>
          </div>
        {/if}
      </section>
    </main>

    <footer class="status">
      {#if tab}
        <span>{tab.file?.format.label ?? ""}</span>
        {#if paged}
          <span class="tnum">Page {paged.page + 1} of {paged.pageCount}</span>
          <span class="tnum">{zoomLabel}</span>
        {:else if image?.natural}
          <span class="tnum">{image.natural.w} × {image.natural.h} px</span>
          <span class="tnum">{Math.round(image.zoom * 100)}%</span>
        {:else if sheet?.mode === "grid" && sheet.infos[sheet.sheet]}
          {@const info = sheet.infos[sheet.sheet]}
          <span class="tnum">{info.rows.toLocaleString()} rows × {info.cols} columns</span>
        {:else if tab.view.kind === "text" && tab.file?.format.id === "code"}
          <span>{tab.view.language}</span>
        {/if}
        {#if tab.file}<span class="tnum">{formatSize(tab.file.size)}</span>{/if}
        <span class="grow"></span>
        {#if tab.view.kind === "office"}
          <span class="badge">{tab.view.status === "converting" ? "Fast preview, rendering exact layout…" : "Fast preview"}</span>
        {:else if sheet}
          {#if sheet.mode === "print" && sheet.converter}
            <span class="badge" class:exact={sheet.print instanceof PagedState}>{sheet.print === "converting" ? "Rendering print layout…" : converterLabel[sheet.converter]}</span>
          {:else}<span class="badge">{sheet.converter ? "Fast preview (grid)" : "Exact"}</span>{/if}
        {:else if tab.view.kind === "paged"}
          {#if officeEngine && msOffice}
            <button class="btn status-btn" onclick={switchConverter}>
              {tab.view.engine.includes("MS Office") ? "Render with LibreOffice" : "Render with MS Office"}
            </button>
          {/if}
          <span class="badge exact">{tab.view.engine}</span>
        {:else if tab.view.kind !== "loading" && tab.view.kind !== "error" && tab.view.kind !== "password"}<span class="badge exact">Exact</span>{/if}
      {/if}
    </footer>
  {:else}
    <section class="empty">
      <div class="drop">
        <Icon name="file" size={28} />
        <h1>Open a file to view it</h1>
        <p class="muted">PDF, Word, Excel, PowerPoint, e-books, images, code and more. Drop files anywhere in this window.</p>
        <button class="btn primary" onclick={chooseFiles}>Open file <span class="kbd">Ctrl+O</span></button>
      </div>
      {#if store.recent.length}
        <div class="recent">
          <h2>Recent files</h2>
          <ul>
            {#each store.recent as path}
              <li>
                <button class="recent-item" title={path} onclick={() => openPath(path)}>
                  <span class="recent-name">{path.split(/[\\/]/).pop()}</span>
                  <span class="recent-dir muted">{path.replace(/[\\/][^\\/]*$/, "")}</span>
                </button>
                <button class="btn" aria-label="Remove {path} from recent files" title="Remove from recent files" onclick={() => forgetRecent(path)}>
                  <Icon name="close" size={14} />
                </button>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    </section>
  {/if}

  {#if dragging}
    <div class="drop-overlay" aria-hidden="true"><div>Drop to open</div></div>
  {/if}
</div>

{#if presenting && paged}
  <div class="presenter" role="dialog" aria-label="Presentation">
    <PagedView state={paged} present onExternalLink={(uri) => (pendingLink = uri)} />
    <div class="presenter-hint">Slide {paged.page + 1} of {paged.pageCount}. Press Esc to exit.</div>
  </div>
{/if}

{#if notice}
  <div class="toast" role="status">{notice}</div>
{/if}

{#if pendingLink}
  {@const link = pendingLink}
  <Dialog title="Open this link in your browser?" onclose={() => (pendingLink = null)}>
    <p class="link-text">{link}</p>
    {#snippet actions()}
      <button class="btn secondary" onclick={() => (pendingLink = null)}>Cancel</button>
      <button
        class="btn primary"
        onclick={() => {
          pendingLink = null;
          void openUrl(link);
        }}>Open link</button
      >
    {/snippet}
  </Dialog>
{/if}

<style>
  .app {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .tabs {
    height: 36px;
    flex: none;
    display: flex;
    align-items: stretch;
    gap: 2px;
    padding: 4px 8px 0;
    background: var(--gray-1);
    overflow-x: auto;
    scrollbar-width: none;
  }
  .tab {
    position: relative;
    display: flex;
    align-items: center;
    max-width: 220px;
    min-width: 96px;
    padding-left: 12px;
    border-radius: 8px 8px 0 0;
    color: var(--text-2);
    animation: tab-in 180ms var(--ease-out);
  }
  .tab.active {
    background: var(--surface);
    color: var(--text);
  }
  .tab.active::after {
    content: "";
    position: absolute;
    left: 12px;
    right: 12px;
    top: 0;
    height: 2px;
    border-radius: 0 0 2px 2px;
    background: var(--accent-9);
  }
  .tab-label {
    flex: 1;
    min-width: 0;
    height: 100%;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tab-close {
    height: 22px;
    min-width: 22px;
    margin: 0 4px;
    padding: 0;
  }
  .new-tab {
    align-self: center;
    margin-left: 4px;
  }
  .toolbar {
    height: 40px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 8px;
    background: var(--surface);
    border-bottom: 1px solid var(--gray-5);
  }
  .sep {
    width: 1px;
    height: 16px;
    margin: 0 6px;
    background: var(--gray-6);
  }
  .grow {
    flex: 1;
  }
  .pager {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .page-input {
    width: 48px;
    height: 26px;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }
  .segmented .btn[aria-checked="true"] {
    color: var(--text);
    background: var(--gray-4);
  }
  .segmented {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 8px;
    background: var(--gray-1);
  }
  .zoom-label {
    min-width: 52px;
    font-variant-numeric: tabular-nums;
  }
  .main {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .stage {
    position: relative;
    flex: 1;
    min-width: 0;
  }
  .status {
    height: 24px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 0 12px;
    font-size: var(--fs-1);
    color: var(--text-2);
    background: var(--surface);
    border-top: 1px solid var(--gray-5);
  }
  .tnum {
    font-variant-numeric: tabular-nums;
  }
  .badge {
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--gray-4);
    color: var(--text);
    animation: fade 200ms var(--ease-out);
  }
  .badge.exact::before {
    content: "";
    display: inline-block;
    width: 6px;
    height: 6px;
    margin-right: 6px;
    border-radius: 50%;
    background: var(--accent-9);
    vertical-align: 1px;
  }
  .swap {
    position: absolute;
    inset: 0;
  }
  .swap.swapped {
    animation: fade 220ms var(--ease-out);
  }
  .swap.with-notes {
    bottom: 132px;
  }
  .notes {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 132px;
    padding: 12px 24px;
    overflow-y: auto;
    background: var(--surface);
    border-top: 1px solid var(--gray-5);
    animation: notes-in 200ms var(--ease-out);
  }
  .notes h2 {
    margin: 0 0 6px;
    font-size: var(--fs-1);
    font-weight: 500;
    color: var(--gray-8);
  }
  .notes p {
    margin: 0;
    max-width: 75ch;
    white-space: pre-wrap;
    color: var(--text-2);
  }
  .presenter {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: #000;
    animation: fade 200ms var(--ease-out);
  }
  .presenter-hint {
    position: absolute;
    right: 16px;
    bottom: 12px;
    padding: 4px 10px;
    border-radius: 999px;
    background: oklch(20% 0.01 65 / 0.7);
    color: var(--gray-11);
    font-size: var(--fs-1);
    font-variant-numeric: tabular-nums;
    opacity: 0;
    transition: opacity 200ms ease;
  }
  .presenter:hover .presenter-hint {
    opacity: 1;
  }
  .toast {
    position: fixed;
    left: 50%;
    bottom: 40px;
    z-index: 60;
    max-width: min(560px, calc(100vw - 32px));
    padding: 10px 16px;
    border-radius: var(--r-float);
    background: var(--gray-4);
    border: 1px solid var(--gray-6);
    box-shadow: var(--shadow-float);
    transform: translateX(-50%);
    animation: toast-in 200ms var(--ease-out);
    overflow-wrap: anywhere;
  }
  .status-btn {
    height: 20px;
    font-size: var(--fs-1);
  }
  @keyframes notes-in {
    from {
      transform: translateY(12px);
      opacity: 0;
    }
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translate(-50%, 8px);
    }
  }
  .fast {
    position: absolute;
    inset: 0;
  }
  .fast.with-banner {
    top: 40px;
  }
  .banner {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 40px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    background: var(--gray-3);
    border-bottom: 1px solid var(--gray-5);
    color: var(--text-2);
  }
  .skeleton-stage {
    position: absolute;
    inset: 0;
    display: flex;
    justify-content: center;
    padding: 16px;
    overflow: hidden;
  }
  .skeleton-page {
    width: min(720px, 100%);
    aspect-ratio: 8.5 / 11;
    padding: 64px 56px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    background: var(--gray-3);
    border-radius: 2px;
    animation: pulse 1.4s ease-in-out infinite;
  }
  .skeleton-page span {
    height: 10px;
    border-radius: 4px;
    background: var(--gray-5);
  }
  .skeleton-page span:first-child {
    width: 40%;
    height: 16px;
    margin-bottom: 12px;
  }
  .skeleton-page span:nth-child(3) {
    width: 85%;
  }
  .skeleton-page span:last-child {
    width: 60%;
  }
  .center-card {
    position: absolute;
    inset: 0;
    margin: auto;
    width: min(440px, calc(100% - 32px));
    height: fit-content;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    padding: 24px;
  }
  .center-card h1,
  .drop h1 {
    margin: 4px 0 0;
    font-size: var(--fs-4);
    font-weight: 600;
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }
  .center-card p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .center-card .field {
    width: 100%;
    margin-top: 8px;
  }
  .row {
    display: flex;
    gap: 8px;
    margin-top: 16px;
  }
  .error {
    color: var(--red-11);
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 32px;
    padding: 32px;
    overflow: auto;
  }
  .drop {
    width: min(520px, 100%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 48px 32px;
    text-align: center;
    border: 1px dashed var(--gray-7);
    border-radius: var(--r-float);
  }
  .drop p {
    margin: 0 0 8px;
    max-width: 48ch;
    text-wrap: pretty;
  }
  .drop .kbd {
    color: oklch(30% 0.04 65);
  }
  .recent {
    width: min(520px, 100%);
  }
  .recent h2 {
    margin: 0 0 8px;
    font-size: var(--fs-2);
    font-weight: 500;
    color: var(--text-2);
  }
  .recent ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .recent li {
    display: flex;
    align-items: center;
    border-radius: var(--r-control);
  }
  .recent-item {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 12px;
    align-items: baseline;
    padding: 6px 8px;
    text-align: left;
  }
  .recent-name {
    flex: none;
    max-width: 60%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .recent-dir {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-1);
  }
  .drop-overlay {
    position: fixed;
    inset: 8px;
    z-index: 40;
    display: grid;
    place-items: center;
    border: 2px dashed var(--accent-9);
    border-radius: var(--r-float);
    background: oklch(16% 0.01 65 / 0.8);
    font-size: var(--fs-3);
    pointer-events: none;
    animation: fade 150ms var(--ease-out);
  }
  .link-text {
    margin: 0;
    padding: 8px 12px;
    border-radius: var(--r-control);
    background: var(--gray-2);
    font-family: ui-monospace, monospace;
    font-size: var(--fs-1);
  }
  @media (hover: hover) and (pointer: fine) {
    .tab:not(.active):hover {
      background: var(--gray-3);
      color: var(--text);
    }
    .recent li:hover {
      background: var(--gray-3);
    }
  }
  @keyframes tab-in {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes pulse {
    50% {
      opacity: 0.6;
    }
  }
</style>
