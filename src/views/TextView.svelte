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
  let chunks = $state<Record<number, string[]>>({ 0: initial.lines });
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
    chunks[chunk] = result.lines;
    total = Math.max(result.total, chunk * CHUNK + result.lines.length);
    complete = result.complete;
  }

  $effect(() => {
    const a = Math.floor(firstLine / CHUNK);
    const b = Math.floor((firstLine + visibleCount) / CHUNK);
    for (let c = a; c <= b; c++) void ensure(c);
  });

  $effect(() => {
    if (complete) return;
    const timer = setInterval(async () => {
      const r = await api.textLines(doc, 0, 1).catch(() => null);
      if (r) {
        total = r.total;
        complete = r.complete;
      }
    }, 250);
    return () => clearInterval(timer);
  });

  const rows = $derived.by(() => {
    const out: { n: number; text: string | null }[] = [];
    for (let n = firstLine; n < Math.min(total, firstLine + visibleCount); n++) {
      out.push({ n, text: chunks[Math.floor(n / CHUNK)]?.[n % CHUNK] ?? null });
    }
    return out;
  });

  function segments(text: string): { t: string; hit: boolean }[] {
    const q = find.query.toLowerCase();
    if (!q) return [{ t: text, hit: false }];
    const lower = text.toLowerCase();
    const out = [];
    let i = 0;
    for (let j = lower.indexOf(q); j >= 0; j = lower.indexOf(q, j + q.length)) {
      if (j > i) out.push({ t: text.slice(i, j), hit: false });
      out.push({ t: text.slice(j, j + q.length), hit: true });
      i = j + q.length;
    }
    if (i < text.length) out.push({ t: text.slice(i), hit: false });
    return out;
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

<div class="scroller" bind:this={scroller} onscroll={onScroll} tabindex="-1" role="document" aria-label="File contents">
  <div class="content" style:height="{height}px">
    <div class="rows" style:transform="translateY({scrollTop + firstLine * ROW - scrollTop * ratio}px)" style:--gutter="{gutter}ch">
      {#each rows as row (row.n)}
        <div class="row" class:current={find.hits[find.index]?.[0] === row.n}>
          <span class="num" aria-hidden="true">{row.n + 1}</span>
          {#if row.text === null}
            <span class="line placeholder"></span>
          {:else}
            <span class="line">{#each segments(row.text) as seg}{#if seg.hit}<mark>{seg.t}</mark>{:else}{seg.t}{/if}{/each}</span>
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
  mark {
    background: var(--hit);
    color: inherit;
    border-radius: 2px;
  }
</style>
