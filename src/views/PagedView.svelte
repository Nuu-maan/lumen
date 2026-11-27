<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { api, pageUrl, tileUrl, type Link, type TextLine } from "../lib/api";
  import { PT_TO_PX, type PagedState } from "../lib/paged.svelte";
  import { openTimes } from "../lib/tabs.svelte";

  let {
    state: s,
    present = false,
    onExternalLink,
  }: { state: PagedState; present?: boolean; onExternalLink: (uri: string) => void } = $props();

  const GAP = 12;
  const PAD = 16;
  const TILE = 512;
  const BASE_WIDTH = 320;

  let scroller: HTMLDivElement;
  let viewW = $state(0);
  let viewH = $state(0);
  let scrollTop = $state(0);
  let scrollLeft = $state(0);
  let dpr = $state(window.devicePixelRatio || 1);
  let settledScale = $state(0);
  let tileRange = $state<[number, number]>([0, -1]);
  let texts = $state<Record<number, (TextLine & { sx: number })[]>>({});
  let links = $state<Record<number, Link[]>>({});
  let settleTimer = 0;

  const pad = $derived(present ? 0 : PAD);
  const gap = $derived(present ? 0 : GAP);
  const rotated = $derived(s.rotation % 180 !== 0);
  const dims = (i: number) => {
    const p = s.pages[i];
    return rotated ? { w: p.h, h: p.w } : { w: p.w, h: p.h };
  };
  const maxW = $derived(Math.max(...s.pages.map((_, i) => dims(i).w)));

  $effect(() => {
    if (!viewW || !viewH || !s.fit) return;
    const fitW = (viewW - pad * 2 - (present ? 0 : 14)) / maxW / PT_TO_PX;
    if (s.fit === "width") {
      s.zoom = Math.max(0.1, Math.min(8, fitW));
    } else {
      const d = dims(untrack(() => s.page));
      const fitH = (viewH - pad * 2) / d.h / PT_TO_PX;
      s.zoom = Math.max(0.1, Math.min(8, Math.min(fitW, (viewW - pad * 2) / d.w / PT_TO_PX, fitH)));
    }
  });

  const css = $derived(s.zoom * PT_TO_PX);
  const shown = $derived(s.mode === "single" || present ? [s.page] : s.pages.map((_, i) => i));

  const layout = $derived.by(() => {
    const tops: number[] = [];
    let y = pad;
    const contentW = Math.max(viewW, maxW * css + pad * 2);
    const slots = shown.map((i) => {
      const d = dims(i);
      const w = d.w * css;
      const h = d.h * css;
      const top = present ? Math.max(0, (viewH - h) / 2) : y;
      const slot = { index: i, top, left: Math.max(pad, (contentW - w) / 2), w, h };
      tops.push(top);
      y += h + gap;
      return slot;
    });
    return { slots, tops, height: y - gap + pad, width: contentW };
  });

  function slotAt(y: number, tops = layout.tops) {
    let lo = 0;
    let hi = tops.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (tops[mid] <= y) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  }

  const visible = $derived.by(() => {
    if (!layout.slots.length) return [0, -1] as const;
    const first = slotAt(scrollTop);
    const last = slotAt(scrollTop + viewH);
    return [first, last] as const;
  });

  // Page frames exist for about two screens above and below the view, so a fast fling on a slow
  // renderer still shows pages (with their small preview) instead of an empty pane.
  const windowStart = $derived(Math.max(0, Math.min(visible[0] - 2, slotAt(scrollTop - viewH * 2))));
  const windowEnd = $derived(Math.max(visible[1] + 3, slotAt(scrollTop + viewH * 3) + 1));

  $effect(() => {
    if (present || s.mode === "single" || !layout.slots.length) return;
    const center = slotAt(scrollTop + viewH / 3);
    const idx = layout.slots[center]?.index ?? 0;
    if (untrack(() => s.page) !== idx) s.page = idx;
  });

  $effect(() => {
    const scale = css * dpr;
    const [first, last] = visible;
    clearTimeout(settleTimer);
    const firstPage = layout.slots[first]?.index ?? 0;
    const lastPage = layout.slots[last]?.index ?? 0;
    const apply = async () => {
      await api.setViewport(s.doc, firstPage, lastPage, scale);
      settledScale = scale;
      tileRange = [first, last];
    };
    if (!viewW) return;
    const settled = untrack(() => settledScale);
    if (settled === scale || settled === 0) void apply();
    else settleTimer = window.setTimeout(apply, 140);
  });

  $effect(() => {
    const j = s.jump;
    if (!j || !scroller) return;
    untrack(() => {
      const slotIndex = shown.indexOf(j.page);
      const slot = layout.slots[slotIndex];
      if (!slot) return;
      if (s.mode === "single" || present) {
        scroller.scrollTop = j.y !== undefined ? Math.max(0, j.y * css - viewH / 3) : 0;
        return;
      }
      const offset = j.y !== undefined && !rotated ? j.y * css - viewH / 3 : -pad / 2;
      const target = slot.top + Math.max(-pad, offset);
      if (j.y !== undefined) {
        const hitTop = slot.top + j.y * css;
        if (hitTop > scroller.scrollTop + 40 && hitTop < scroller.scrollTop + viewH - 60) return;
      }
      scroller.scrollTop = target;
      scrollTop = scroller.scrollTop;
    });
  });

  type Anchor = { page: number; fy: number; fx: number; cy: number; cx: number; pinTop: boolean };
  let zoomAnchor: Anchor | null = null;
  let restoreTo: Anchor | null = null;
  let lastLayout: typeof layout | null = null;

  function anchorAt(cx: number, cy: number, l = layout, pinTop = false): Anchor | null {
    const top = scroller.scrollTop;
    const left = scroller.scrollLeft;
    const slot = l.slots[slotAt(top + cy, l.tops)];
    if (!slot) return null;
    return { page: slot.index, fy: (top + cy - slot.top) / slot.h, fx: (left + cx - slot.left) / slot.w, cy, cx, pinTop };
  }

  $effect.pre(() => {
    const prev = lastLayout;
    lastLayout = layout;
    if (!scroller || !prev || present) return;
    const a = zoomAnchor ?? restoreTo ?? untrack(() => anchorAt(viewW / 2, viewH / 3, prev, s.fit === "page"));
    zoomAnchor = null;
    if (!a) return;
    const queued = restoreTo !== null;
    restoreTo = a;
    if (queued) return;
    queueMicrotask(() => {
      const a = restoreTo!;
      restoreTo = null;
      const slot = layout.slots[shown.indexOf(a.page)];
      if (!slot) return;
      scroller.scrollTop = a.pinTop ? slot.top - pad / 2 : slot.top + a.fy * slot.h - a.cy;
      scroller.scrollLeft = slot.left + a.fx * slot.w - a.cx;
      scrollTop = scroller.scrollTop;
      scrollLeft = scroller.scrollLeft;
    });
  });

  export function zoomTo(zoom: number, cx = viewW / 2, cy = viewH / 2) {
    zoomAnchor = anchorAt(cx, cy);
    s.setZoom(zoom);
  }

  function onWheel(e: WheelEvent) {
    if (!e.ctrlKey) return;
    e.preventDefault();
    const rect = scroller.getBoundingClientRect();
    const factor = Math.exp(-e.deltaY * (e.deltaMode === 1 ? 0.05 : 0.0025));
    zoomTo(s.zoom * factor, e.clientX - rect.left, e.clientY - rect.top);
  }

  let scrollFrame = 0;
  function onScroll() {
    if (scrollFrame) return;
    scrollFrame = requestAnimationFrame(() => {
      scrollFrame = 0;
      scrollTop = scroller.scrollTop;
      scrollLeft = scroller.scrollLeft;
    });
  }

  function tilesFor(slot: { index: number; top: number; left: number; w: number; h: number }) {
    const scale = settledScale;
    if (!scale) return [];
    const p = s.pages[slot.index];
    const pw = Math.ceil(p.w * scale);
    const ph = Math.ceil(p.h * scale);
    const k = scale / dpr / css;
    const vx0 = (scrollLeft - slot.left - 256) * k * dpr;
    const vy0 = (scrollTop - slot.top - 256) * k * dpr;
    const vx1 = vx0 + (viewW + 512) * k * dpr;
    const vy1 = vy0 + (viewH + 512) * k * dpr;
    const sw = slot.w * k * dpr;
    const sh = slot.h * k * dpr;
    const corners = [
      [vx0, vy0],
      [vx1, vy0],
      [vx0, vy1],
      [vx1, vy1],
    ].map(([u, v]) => {
      switch (s.rotation) {
        case 90:
          return [v, sw - u];
        case 180:
          return [sw - u, sh - v];
        case 270:
          return [sh - v, u];
        default:
          return [u, v];
      }
    });
    const x0 = Math.max(0, Math.min(...corners.map((c) => c[0])));
    const y0 = Math.max(0, Math.min(...corners.map((c) => c[1])));
    const x1 = Math.min(pw, Math.max(...corners.map((c) => c[0])));
    const y1 = Math.min(ph, Math.max(...corners.map((c) => c[1])));
    const tiles = [];
    for (let ty = Math.floor(y0 / TILE) * TILE; ty < y1; ty += TILE) {
      for (let tx = Math.floor(x0 / TILE) * TILE; tx < x1; tx += TILE) {
        const w = Math.min(TILE, pw - tx);
        const h = Math.min(TILE, ph - ty);
        tiles.push({ key: `${tx},${ty}`, url: tileUrl(s.doc, slot.index, scale, tx, ty, w, h), x: tx, y: ty, w, h });
      }
    }
    return tiles;
  }

  const measure = document.createElement("canvas").getContext("2d")!;
  measure.font = "100px sans-serif";

  async function loadPageExtras(page: number) {
    if (!texts[page]) {
      texts[page] = [];
      const lines = await api.pageText(s.doc, page).catch(() => []);
      texts[page] = lines.map((l) => {
        const w100 = measure.measureText(l.text).width;
        return { ...l, sx: w100 > 0 ? (l.rect.w * 100) / (w100 * l.rect.h) : 1 };
      });
    }
    if (!links[page]) {
      links[page] = [];
      links[page] = await api.pageLinks(s.doc, page).catch(() => []);
    }
  }

  $effect(() => {
    const [first, last] = tileRange;
    for (let i = first; i <= last; i++) {
      const slot = layout.slots[i];
      if (slot) untrack(() => loadPageExtras(slot.index));
    }
  });

  function innerTransform(w: number, h: number) {
    switch (s.rotation) {
      case 90:
        return `translate(${w}px,0) rotate(90deg)`;
      case 180:
        return `translate(${w}px,${h}px) rotate(180deg)`;
      case 270:
        return `translate(0,${h}px) rotate(270deg)`;
      default:
        return "";
    }
  }

  function onCopy(e: ClipboardEvent) {
    const sel = getSelection();
    if (!sel?.rangeCount || sel.isCollapsed || !e.clipboardData) return;
    const range = sel.getRangeAt(0);
    const lines: string[] = [];
    for (const span of scroller.querySelectorAll(".text-layer span")) {
      const text = span.firstChild;
      if (!text || !range.intersectsNode(text)) continue;
      const part = document.createRange();
      part.selectNodeContents(text);
      if (range.compareBoundaryPoints(Range.START_TO_START, part) > 0) part.setStart(range.startContainer, range.startOffset);
      if (range.compareBoundaryPoints(Range.END_TO_END, part) < 0) part.setEnd(range.endContainer, range.endOffset);
      lines.push(part.toString());
    }
    const copied = lines.filter(Boolean).join("\n");
    if (!copied) return;
    e.preventDefault();
    e.clipboardData.setData("text/plain", copied);
  }

  function followLink(link: Link, e: MouseEvent) {
    e.preventDefault();
    if (link.page !== null) s.goTo(link.page);
    else if (link.uri) onExternalLink(link.uri);
  }

  const reported = new Set<string>();

  function reportFirstPage(name: "first_page_preview" | "first_page_sharp") {
    const started = openTimes.get(-s.doc);
    if (started === undefined || reported.has(name)) return;
    reported.add(name);
    void api.benchMark(name, performance.now() - started);
    if (name === "first_page_sharp") openTimes.delete(-s.doc);
  }

  function onTileLoad(e: Event) {
    const layer = (e.currentTarget as HTMLElement).parentElement!;
    if ([...layer.querySelectorAll("img")].every((i) => i.complete && i.naturalWidth)) reportFirstPage("first_page_sharp");
  }

  function retryTile(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    if (img.dataset.retried) return;
    img.dataset.retried = "1";
    setTimeout(() => {
      if (img.isConnected) img.src = img.src + "?r";
    }, 120);
  }

  onMount(() => {
    const ro = new ResizeObserver(() => {
      viewW = scroller.clientWidth;
      viewH = scroller.clientHeight;
    });
    ro.observe(scroller);
    const media = matchMedia(`(resolution: ${dpr}dppx)`);
    const onDpr = () => (dpr = window.devicePixelRatio || 1);
    media.addEventListener("change", onDpr);
    return () => {
      ro.disconnect();
      media.removeEventListener("change", onDpr);
    };
  });

  const hitsByPage = $derived.by(() => {
    const map: Record<number, { rect: { x: number; y: number; w: number; h: number }; current: boolean }[]> = {};
    s.hits.forEach((h, i) => (map[h.page] ??= []).push({ rect: h.rect, current: i === s.hitIndex }));
    return map;
  });
</script>

<div
  class="scroller"
  class:present
  class:invert={s.invert}
  bind:this={scroller}
  onscroll={onScroll}
  onwheel={onWheel}
  oncopy={onCopy}
  tabindex="-1"
  role="document"
  aria-label="Document pages"
>
  <div class="content" style:height="{layout.height}px" style:width="{layout.width}px">
    {#each layout.slots.slice(windowStart, windowEnd) as slot, k (slot.index)}
      {@const p = s.pages[slot.index]}
      {@const sharp = tileRange[0] <= windowStart + k && windowStart + k <= tileRange[1]}
      <div class="slot" style:top="{slot.top}px" style:left="{slot.left}px" style:width="{slot.w}px" style:height="{slot.h}px">
        <div
          class="page"
          style:width="{p.w * css}px"
          style:height="{p.h * css}px"
          style:transform={innerTransform(slot.w, slot.h)}
        >
          <img class="base" src={pageUrl(s.doc, slot.index, BASE_WIDTH)} alt="" draggable="false" onload={() => reportFirstPage("first_page_preview")} />
          {#if sharp && settledScale}
            <div class="tiles" style:transform="scale({(css * dpr) / settledScale})">
              {#each tilesFor(slot) as t (t.key + settledScale)}
                <img
                  src={t.url}
                  alt=""
                  draggable="false"
                  onerror={retryTile}
                  onload={onTileLoad}
                  style:left="{t.x / dpr}px"
                  style:top="{t.y / dpr}px"
                  style:width="{t.w / dpr}px"
                  style:height="{t.h / dpr}px"
                />
              {/each}
            </div>
          {/if}
          {#if hitsByPage[slot.index]}
            {#each hitsByPage[slot.index] as h}
              <div
                class="hit"
                class:current={h.current}
                style:left="{h.rect.x * css}px"
                style:top="{h.rect.y * css}px"
                style:width="{h.rect.w * css}px"
                style:height="{h.rect.h * css}px"
              ></div>
            {/each}
          {/if}
          {#if sharp && texts[slot.index]}
            <div class="text-layer">
              {#each texts[slot.index] as line}
                <span
                  style:left="{line.rect.x * css}px"
                  style:top="{line.rect.y * css}px"
                  style:font-size="{line.rect.h * css}px"
                  style:transform="scaleX({line.sx})">{line.text}</span
                >
              {/each}
            </div>
          {/if}
          {#if sharp && links[slot.index]?.length}
            {#each links[slot.index] as link}
              <a
                class="link"
                href={link.uri ?? `#page-${(link.page ?? 0) + 1}`}
                title={link.uri ?? `Go to page ${(link.page ?? 0) + 1}`}
                onclick={(e) => followLink(link, e)}
                style:left="{link.rect.x * css}px"
                style:top="{link.rect.y * css}px"
                style:width="{link.rect.w * css}px"
                style:height="{link.rect.h * css}px"
              ></a>
            {/each}
          {/if}
        </div>
      </div>
    {/each}
  </div>
</div>

<style>
  .scroller {
    position: absolute;
    inset: 0;
    overflow: auto;
    outline: none;
    background: var(--desk);
  }
  .scroller.present {
    background: #000;
    overflow: hidden;
  }
  .content {
    position: relative;
  }
  .slot {
    position: absolute;
  }
  .page {
    position: absolute;
    top: 0;
    left: 0;
    transform-origin: 0 0;
    background: #fff;
    box-shadow: var(--shadow-page);
    border-radius: 2px;
    overflow: hidden;
  }
  .present .page {
    box-shadow: none;
    border-radius: 0;
  }
  .invert .page {
    filter: invert(0.92) hue-rotate(180deg);
  }
  .base {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    user-select: none;
  }
  .tiles {
    position: absolute;
    inset: 0;
    transform-origin: 0 0;
  }
  .tiles img {
    position: absolute;
    user-select: none;
  }
  .text-layer {
    position: absolute;
    inset: 0;
    line-height: 1;
    font-family: sans-serif;
  }
  .text-layer span {
    position: absolute;
    white-space: pre;
    color: transparent;
    transform-origin: 0 0;
    cursor: text;
  }
  .text-layer ::selection {
    background: oklch(60% 0.15 240 / 0.35);
  }
  .hit {
    position: absolute;
    background: var(--hit);
    mix-blend-mode: multiply;
    border-radius: 2px;
  }
  .hit.current {
    background: var(--hit-current);
    outline: 2px solid oklch(65% 0.18 50);
  }
  .link {
    position: absolute;
    z-index: 2;
    cursor: pointer;
  }
  @media (hover: hover) and (pointer: fine) {
    .link:hover {
      background: oklch(60% 0.15 240 / 0.12);
    }
  }
</style>
