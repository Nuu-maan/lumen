<script lang="ts">
  import { onMount } from "svelte";
  import { api, type CellStyle, type SheetInfo } from "../lib/api";
  import { openTimes } from "../lib/tabs.svelte";

  let {
    doc,
    names,
    info,
    sheet,
    onselect,
  }: { doc: number; names: string[]; info: SheetInfo; sheet: number; onselect: (sheet: number) => void } = $props();

  const ROW_H = 24;
  const HEAD_W = 56;
  const HEAD_H = 24;
  const BLOCK_ROWS = 200;
  const BLOCK_COLS = 48;

  let scroller: HTMLDivElement;
  let viewW = $state(0);
  let viewH = $state(0);
  let scrollTop = $state(0);
  let scrollLeft = $state(0);
  let blocks = $state<Record<string, string[][]>>({});
  let looks = $state<Record<string, CellStyle>>({});
  let merges = $state<[number, number, number, number][]>([]);
  let widths = $state<number[]>([]);
  const pending = new Set<string>();
  let generation = 0;

  const rows = $derived(Math.max(info.rows, 1));
  const cols = $derived(Math.max(info.cols, 1));
  const fr = $derived(Math.min(info.frozenRows, rows));
  const fc = $derived(Math.min(info.frozenCols, cols));

  const xs = $derived.by(() => {
    const out = new Float64Array(cols + 1);
    for (let c = 0; c < cols; c++) out[c + 1] = out[c] + (widths[c] ?? 96);
    return out;
  });
  const frozenW = $derived(xs[fc]);
  const frozenH = $derived(fr * ROW_H);

  function colName(c: number) {
    let s = "";
    for (c += 1; c > 0; c = Math.floor((c - 1) / 26)) s = String.fromCharCode(65 + ((c - 1) % 26)) + s;
    return s;
  }

  function colAt(x: number) {
    let lo = 0;
    let hi = cols - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (xs[mid] <= x) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  }

  const visRows = $derived.by(() => {
    const first = Math.max(fr, Math.floor((scrollTop + frozenH) / ROW_H));
    const last = Math.min(rows - 1, Math.ceil((scrollTop + viewH) / ROW_H) + 2);
    return [first, last] as const;
  });
  const visCols = $derived.by(() => {
    const first = Math.max(fc, colAt(scrollLeft + frozenW));
    const last = Math.min(cols - 1, colAt(scrollLeft + viewW) + 1);
    return [first, last] as const;
  });

  async function ensure(br: number, bc: number) {
    const key = `${sheet}/${br}/${bc}`;
    if (blocks[key] || pending.has(key)) return;
    pending.add(key);
    const gen = generation;
    const r0 = br * BLOCK_ROWS;
    const c0 = bc * BLOCK_COLS;
    const data = await api.sheetCells(doc, sheet, [r0, Math.min(rows, r0 + BLOCK_ROWS)], [c0, Math.min(cols, c0 + BLOCK_COLS)]).catch(() => null);
    if (gen !== generation) return;
    pending.delete(key);
    if (!data) return;
    blocks[key] = data.values;
    for (const s of data.styles) looks[`${sheet}/${s.r}/${s.c}`] = s;
    if (br === 0) estimateWidths(c0, data.values);
  }

  function estimateWidths(c0: number, data: string[][]) {
    const next = widths.slice();
    for (let c = 0; c < (data[0]?.length ?? 0); c++) {
      let longest = 0;
      for (const row of data) longest = Math.max(longest, Math.min(48, row[c]?.split("\n")[0].length ?? 0));
      next[c0 + c] = Math.max(56, Math.min(320, longest * 7.5 + 20));
    }
    widths = next;
  }

  // Each merged range draws once, from its top-left cell, across all the cells it covers.
  // shortcut: a merge whose top-left cell is scrolled out of view isn't drawn; fine for titles and headers, revisit for tall merges.
  const merged = $derived.by(() => {
    const anchors = new Map<string, [number, number]>();
    const covered = new Set<string>();
    for (const [r0, c0, r1, c1] of merges) {
      anchors.set(`${r0}/${c0}`, [r1, c1]);
      for (let r = r0; r <= r1; r++) for (let c = c0; c <= c1; c++) if (r !== r0 || c !== c0) covered.add(`${r}/${c}`);
    }
    return { anchors, covered };
  });

  function value(r: number, c: number) {
    return blocks[`${sheet}/${Math.floor(r / BLOCK_ROWS)}/${Math.floor(c / BLOCK_COLS)}`]?.[r % BLOCK_ROWS]?.[c % BLOCK_COLS];
  }

  $effect(() => {
    const ranges: [number, number][] = [[visRows[0], visRows[1]]];
    if (fr) ranges.push([0, fr - 1]);
    const colRanges: [number, number][] = [[visCols[0], visCols[1]]];
    if (fc) colRanges.push([0, fc - 1]);
    for (const [ra, rb] of ranges)
      for (const [ca, cb] of colRanges)
        for (let br = Math.floor(ra / BLOCK_ROWS); br <= Math.floor(rb / BLOCK_ROWS); br++)
          for (let bc = Math.floor(ca / BLOCK_COLS); bc <= Math.floor(cb / BLOCK_COLS); bc++) void ensure(br, bc);
  });

  // Each sheet keeps its own scroll position when you switch away and back.
  const positions = new Map<number, [number, number]>();
  let shownSheet = -1;
  $effect(() => {
    const next = sheet;
    if (!scroller) return;
    if (shownSheet >= 0) positions.set(shownSheet, [scroller.scrollTop, scroller.scrollLeft]);
    shownSheet = next;
    widths = [];
    merges = [];
    // Formatting loads after the values; when it arrives the visible cells are fetched again with it.
    void api.sheetStyles(doc, next).then(
      (s) => {
        if (next !== sheet) return;
        merges = s.merges;
        if (!s.styled) return;
        generation++;
        pending.clear();
        blocks = {};
      },
      () => {},
    );
    const [top, left] = positions.get(next) ?? [0, 0];
    scroller.scrollTop = top;
    scroller.scrollLeft = left;
  });

  // When horizontal scrolling stops part-way through a column, settle on the nearest column edge,
  // so no sliver of a column hides under the frozen ones.
  let settle = 0;
  function settleColumns() {
    clearTimeout(settle);
    settle = window.setTimeout(() => {
      const x = scroller.scrollLeft + frozenW;
      const c = colAt(x);
      const start = xs[c];
      const end = xs[c + 1];
      if (x === start || c >= cols - 1) return;
      const target = (x - start < end - x ? start : end) - frozenW;
      const instant = matchMedia("(prefers-reduced-motion: reduce)").matches;
      scroller.scrollTo({ left: Math.max(0, target), behavior: instant ? "auto" : "smooth" });
    }, 150);
  }

  const range = (a: number, b: number) => Array.from({ length: Math.max(0, b - a + 1) }, (_, i) => a + i);
  // Numbers, including formatted ones like $1,250.00, (1,500.00) and 12.4%, are right-aligned like in Excel.
  const isNumeric = (v: string | undefined) => !!v && /^[-(]?[$€£¥₹]? ?[0-9.,]*[0-9][0-9.,]*%?[)]? ?$/.test(v);

  let frame = 0;
  function onScroll() {
    if (scroller.scrollLeft !== scrollLeft) settleColumns();
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      scrollTop = scroller.scrollTop;
      scrollLeft = scroller.scrollLeft;
    });
  }

  onMount(() => {
    const started = openTimes.get(-doc);
    if (started !== undefined) {
      openTimes.delete(-doc);
      const wait = () => (blocks[`${sheet}/0/0`] ? api.benchMark("sheet_grid", performance.now() - started) : requestAnimationFrame(wait));
      requestAnimationFrame(wait);
    }
    const ro = new ResizeObserver(() => {
      viewW = scroller.clientWidth - HEAD_W;
      viewH = scroller.clientHeight - HEAD_H;
    });
    ro.observe(scroller);
    return () => ro.disconnect();
  });
</script>

{#snippet cellAt(r: number, c: number, left: number, top: number)}
  {#if !merged.covered.has(`${r}/${c}`)}
    {@const v = value(r, c)}
    {@const look = looks[`${sheet}/${r}/${c}`]}
    {@const span = merged.anchors.get(`${r}/${c}`)}
    <div
      class="cell"
      class:num={isNumeric(v)}
      class:merged={!!span}
      style:left="{left}px"
      style:top="{top}px"
      style:width="{span ? xs[Math.min(span[1], cols - 1) + 1] - xs[c] : (widths[c] ?? 96)}px"
      style:height={span ? `${(span[0] - r + 1) * ROW_H}px` : undefined}
      style:color={look?.color}
      style:background-color={look?.fill ?? (span ? "#fff" : undefined)}
      style:font-weight={look?.bold ? 600 : undefined}
      title={v && v.length > 24 ? v : undefined}
    >
      {v ?? ""}
    </div>
  {/if}
{/snippet}

<div class="sheet-view">
  <div class="grid" bind:this={scroller} onscroll={onScroll} tabindex="-1" role="grid" aria-rowcount={rows} aria-colcount={cols}>
    <div class="stage" style:width="{viewW + HEAD_W}px" style:height="{viewH + HEAD_H}px">
    <div class="pane body" style:left="{HEAD_W + frozenW}px" style:top="{HEAD_H + frozenH}px">
      <div style:transform="translate({-scrollLeft - frozenW}px, {-scrollTop - frozenH}px)">
        {#each range(visRows[0], visRows[1]) as r (r)}
          {#each range(visCols[0], visCols[1]) as c (c)}{@render cellAt(r, c, xs[c], r * ROW_H)}{/each}
        {/each}
      </div>
    </div>
    {#if fr}
      <div class="pane frozen-rows" style:left="{HEAD_W + frozenW}px" style:top="{HEAD_H}px" style:height="{frozenH}px">
        <div style:transform="translate({-scrollLeft - frozenW}px, 0)">
          {#each range(0, fr - 1) as r (r)}
            {#each range(visCols[0], visCols[1]) as c (c)}{@render cellAt(r, c, xs[c], r * ROW_H)}{/each}
          {/each}
        </div>
      </div>
    {/if}
    {#if fc}
      <div class="pane frozen-cols" style:left="{HEAD_W}px" style:top="{HEAD_H + frozenH}px" style:width="{frozenW}px">
        <div style:transform="translate(0, {-scrollTop - frozenH}px)">
          {#each range(visRows[0], visRows[1]) as r (r)}
            {#each range(0, fc - 1) as c (c)}{@render cellAt(r, c, xs[c], r * ROW_H)}{/each}
          {/each}
        </div>
      </div>
    {/if}
    {#if fr && fc}
      <div class="pane corner" style:left="{HEAD_W}px" style:top="{HEAD_H}px" style:width="{frozenW}px" style:height="{frozenH}px">
        {#each range(0, fr - 1) as r (r)}
          {#each range(0, fc - 1) as c (c)}{@render cellAt(r, c, xs[c], r * ROW_H)}{/each}
        {/each}
      </div>
    {/if}
    <div class="pane col-heads" style:left="{HEAD_W}px">
      <div style:transform="translate({-scrollLeft}px, 0)">
        {#each [...range(visCols[0], visCols[1]), ...range(0, fc - 1)] as c (c)}
          <div class="head" class:frozen={c < fc} style:left="{c < fc ? xs[c] + scrollLeft : xs[c]}px" style:width="{widths[c] ?? 96}px">{colName(c)}</div>
        {/each}
      </div>
    </div>
    <div class="pane row-heads" style:top="{HEAD_H}px">
      <div style:transform="translate(0, {-scrollTop}px)">
        {#each [...range(visRows[0], visRows[1]), ...range(0, fr - 1)] as r (r)}
          <div class="head" class:frozen={r < fr} style:top="{r < fr ? r * ROW_H + scrollTop : r * ROW_H}px">{r + 1}</div>
        {/each}
      </div>
    </div>
    <div class="pane top-left"></div>
    {#if info.rows === 0}
      <p class="empty muted">This sheet is empty.</p>
    {/if}
    </div>
    <div class="spacer" style:width="{xs[cols] + HEAD_W}px" style:height="{rows * ROW_H + HEAD_H}px" style:margin-top="-{viewH + HEAD_H}px"></div>
  </div>
  {#if names.length > 1}
    <div class="tabs" role="tablist" aria-label="Sheets">
      {#each names as name, i}
        <button class="sheet-tab" role="tab" aria-selected={i === sheet} onclick={() => onselect(i)}>{name}</button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .sheet-view {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    background: #fff;
  }
  .grid {
    position: relative;
    flex: 1;
    overflow: auto;
    outline: none;
    font: 13px/24px Calibri, Carlito, "Segoe UI", system-ui, sans-serif;
    color: #1d1d1f;
  }
  .stage {
    position: sticky;
    top: 0;
    left: 0;
    z-index: 1;
    overflow: hidden;
  }
  .spacer {
    pointer-events: none;
  }
  .pane {
    position: absolute;
    overflow: hidden;
  }
  .head.frozen {
    z-index: 1;
  }
  .body {
    right: 0;
    bottom: 0;
  }
  .frozen-rows {
    right: 0;
    border-bottom: 2px solid #9aa0a6;
    background: #fff;
  }
  .frozen-cols {
    bottom: 0;
    border-right: 2px solid #9aa0a6;
    background: #fff;
  }
  .corner {
    background: #fff;
    border-right: 2px solid #9aa0a6;
    border-bottom: 2px solid #9aa0a6;
  }
  .cell,
  .head {
    position: absolute;
    height: 24px;
    padding: 0 6px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    border-right: 1px solid #e3e3e3;
    border-bottom: 1px solid #e3e3e3;
  }
  .cell.num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .cell.merged {
    z-index: 1;
  }
  .col-heads {
    top: 0;
    right: 0;
    height: 24px;
    background: #f3f2f1;
    border-bottom: 1px solid #c8c6c4;
  }
  .row-heads {
    left: 0;
    bottom: 0;
    width: 56px;
    background: #f3f2f1;
    border-right: 1px solid #c8c6c4;
  }
  .head {
    color: #605e5c;
    text-align: center;
    font-size: 12px;
    background: #f3f2f1;
  }
  .row-heads .head {
    width: 56px;
  }
  .top-left {
    top: 0;
    left: 0;
    width: 56px;
    height: 24px;
    background: #e8e6e4;
    border-right: 1px solid #c8c6c4;
    border-bottom: 1px solid #c8c6c4;
  }
  .empty {
    position: absolute;
    top: 48px;
    left: 72px;
    color: #605e5c;
  }
  .tabs {
    flex: none;
    display: flex;
    gap: 2px;
    padding: 0 8px;
    height: 32px;
    align-items: stretch;
    background: var(--surface);
    border-top: 1px solid var(--gray-5);
    overflow-x: auto;
  }
  .sheet-tab {
    padding: 0 16px;
    color: var(--text-2);
    border-radius: 0 0 6px 6px;
    white-space: nowrap;
  }
  .sheet-tab[aria-selected="true"] {
    background: var(--gray-4);
    color: var(--text);
    box-shadow: inset 0 2px 0 var(--accent-9);
  }
  @media (hover: hover) and (pointer: fine) {
    .sheet-tab:hover {
      color: var(--text);
    }
  }
</style>
