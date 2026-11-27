<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { api, type Lines } from "../lib/api";
  import { openTimes } from "../lib/tabs.svelte";

  export interface TextFind {
    query: string;
    hits: [number, number][];
    index: number;
    searching: boolean;
  }

  let {
    doc,
    first,
    find,
  }: { doc: number; first: Lines; find: TextFind } = $props();

  const ROW = 20;
  const CHUNK = 200;
  const MAX_HEIGHT = 8_000_000;

  let scroller: HTMLDivElement;
  let viewH = $state(0);
  let scrollTop = $state(0);
  const initial = untrack(() => first);
  let total = $state(initial.total);
  let complete = $state(initial.complete);
  let highlighting = $state(initial.highlighting);
  type Chunk = { lines: string[]; tokens: [number, number][][] | null };
  let chunks = $state<Record<number, Chunk>>({ 0: { lines: initial.lines, tokens: initial.tokens } });
  const loading = new Set<number>();

  const fullHeight = $derived(total * ROW + 16);
  const height = $derived(Math.min(fullHeight, MAX_HEIGHT));
  const ratio = $derived(fullHeight > MAX_HEIGHT ? (fullHeight - viewH) / Math.max(1, MAX_HEIGHT - viewH) : 1);
  const firstLine = $derived(Math.max(0, Math.floor((scrollTop * ratio) / ROW) - 5));
  const visibleCount = $derived(Math.ceil(viewH / ROW) + 10);
  const gutter = $derived(String(total).length);

  async function ensure(chunk: number) {
    if (chunks[chunk] || loading.has(chunk)) return;
    loading.add(chunk);
    const result = await api.textLines(doc, chunk * CHUNK, CHUNK).catch(() => null);
    loading.delete(chunk);
    if (!result) return;
    chunks[chunk] = { lines: result.lines, tokens: result.tokens };
    total = Math.max(result.total, chunk * CHUNK + result.lines.length);
    complete = result.complete;
  }

  $effect(() => {
    const a = Math.floor(firstLine / CHUNK);
    const b = Math.floor((firstLine + visibleCount) / CHUNK);
    for (let c = a; c <= b; c++) void ensure(c);
  });

  $effect(() => {
    if (complete && !highlighting) return;
    const timer = setInterval(async () => {
      const r = await api.textLines(doc, 0, 1).catch(() => null);
      if (!r) return;
      total = r.total;
      complete = r.complete;
      if (highlighting && !r.highlighting) {
        highlighting = false;
        chunks = {};
      }
    }, 150);
    return () => clearInterval(timer);
  });

  const rows = $derived.by(() => {
    const out: { n: number; text: string | null; tokens: [number, number][] | null }[] = [];
    for (let n = firstLine; n < Math.min(total, firstLine + visibleCount); n++) {
      const chunk = chunks[Math.floor(n / CHUNK)];
      out.push({ n, text: chunk?.lines[n % CHUNK] ?? null, tokens: chunk?.tokens?.[n % CHUNK] ?? null });
    }
    return out;
  });

  function segments(text: string, tokens: [number, number][] | null): { t: string; cls: number; hit: boolean }[] {
    const cuts = new Set([0, text.length]);
    const classAt: [number, number][] = [];
    let at = 0;
    for (const [len, cls] of tokens ?? []) {
      classAt.push([at, cls]);
      cuts.add(at);
      at += len;
      cuts.add(Math.min(at, text.length));
    }
    const hits: [number, number][] = [];
    const q = find.query.toLowerCase();
    if (q) {
      const lower = text.toLowerCase();
      for (let j = lower.indexOf(q); j >= 0; j = lower.indexOf(q, j + q.length)) {
        hits.push([j, j + q.length]);
        cuts.add(j).add(j + q.length);
      }
    }
    const points = [...cuts].filter((p) => p <= text.length).sort((a, b) => a - b);
    const out = [];
    for (let k = 0; k < points.length - 1; k++) {
      const [a, b] = [points[k], points[k + 1]];
      if (a === b) continue;
      const cls = classAt.findLast(([start]) => start <= a)?.[1] ?? 0;
      out.push({ t: text.slice(a, b), cls, hit: hits.some(([s, e]) => a >= s && b <= e) });
    }
    return out;
  }

  // Ctrl+Home and Ctrl+End jump to the first and last line; plain Home and End already scroll natively.
  function onkeydown(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || (e.target as HTMLElement)?.closest?.("input, textarea")) return;
    if (e.key === "Home") scroller.scrollTop = 0;
    else if (e.key === "End") scroller.scrollTop = scroller.scrollHeight;
    else return;
    e.preventDefault();
  }

  export function goToLine(line: number) {
    const y = (line * ROW) / ratio - viewH / 3;
    scroller.scrollTop = Math.max(0, y);
  }

  $effect(() => {
    const hit = find.hits[find.index];
    if (!hit) return;
    const top = hit[0] * ROW;
    const visibleTop = scrollTop * ratio;
    if (top < visibleTop || top > visibleTop + viewH - ROW * 2) goToLine(hit[0]);
  });

  let frame = 0;
  function onScroll() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      scrollTop = scroller.scrollTop;
    });
  }

  onMount(() => {
    const started = openTimes.get(-doc);
    if (started !== undefined) {
      openTimes.delete(-doc);
      requestAnimationFrame(() => void api.benchMark("text_first_screen", performance.now() - started));
    }
    const ro = new ResizeObserver(() => (viewH = scroller.clientHeight));
    ro.observe(scroller);
    return () => ro.disconnect();
  });
</script>

<svelte:window {onkeydown} />

<div class="scroller" bind:this={scroller} onscroll={onScroll} tabindex="-1" role="document" aria-label="File contents">
  <div class="content" style:height="{height}px">
    <div class="rows" style:transform="translateY({scrollTop + firstLine * ROW - scrollTop * ratio}px)" style:--gutter="{gutter}ch">
      {#each rows as row (row.n)}
        <div class="row" class:current={find.hits[find.index]?.[0] === row.n}>
          <span class="num" aria-hidden="true">{row.n + 1}</span>
          {#if row.text === null}
            <span class="line placeholder"></span>
          {:else}
            <span class="line">{#each segments(row.text, row.tokens) as seg}{#if seg.hit}<mark class="t{seg.cls}">{seg.t}</mark>{:else if seg.cls}<span class="t{seg.cls}">{seg.t}</span>{:else}{seg.t}{/if}{/each}</span>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .scroller {
    position: absolute;
    inset: 0;
    overflow: auto;
    background: var(--gray-1);
    outline: none;
  }
  .content {
    position: relative;
    min-width: 100%;
  }
  .rows {
    position: absolute;
    top: 8px;
    left: 0;
    min-width: 100%;
    font-family: ui-monospace, "Cascadia Mono", Consolas, "SF Mono", Menlo, monospace;
    font-size: 13px;
    line-height: 20px;
    tab-size: 4;
  }
  .row {
    display: flex;
    height: 20px;
    white-space: pre;
  }
  .row.current {
    background: var(--gray-3);
  }
  .num {
    flex: none;
    width: calc(var(--gutter) + 32px);
    padding-right: 16px;
    text-align: right;
    color: var(--gray-8);
    user-select: none;
    font-variant-numeric: tabular-nums;
  }
  .line {
    padding-right: 32px;
    color: var(--gray-12);
  }
  .placeholder {
    width: 40%;
    margin: 6px 0;
    border-radius: 4px;
    background: var(--gray-3);
  }
  .t1 { color: oklch(62% 0.02 65); font-style: italic; }
  .t2 { color: oklch(80% 0.11 140); }
  .t3, .t4 { color: oklch(80% 0.1 50); }
  .t5 { color: oklch(76% 0.12 340); }
  .t6 { color: oklch(82% 0.1 230); }
  .t7, .t8 { color: oklch(82% 0.09 190); }
  .t9, .t10 { color: oklch(84% 0.1 85); }
  mark {
    background: var(--hit);
    color: inherit;
    border-radius: 2px;
  }
</style>
