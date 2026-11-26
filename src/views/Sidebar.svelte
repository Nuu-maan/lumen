<script lang="ts">
  import { pageUrl, type OutlineItem } from "../lib/api";
  import type { PagedState } from "../lib/paged.svelte";
  import { kbd } from "../lib/format";

  let { state: s }: { state: PagedState } = $props();
  let panel = $state<"pages" | "outline">("pages");
  let list: HTMLDivElement | undefined = $state();

  const THUMB = 132;

  $effect(() => {
    if (panel === "outline") void s.loadOutline();
  });

  $effect(() => {
    const current = list?.querySelector<HTMLElement>(`[data-page="${s.page}"]`);
    current?.scrollIntoView({ block: "nearest" });
  });

  const thumbH = (i: number) => Math.round((THUMB * s.pages[i].h) / s.pages[i].w);
</script>

<aside class="sidebar">
  <div class="switch" role="tablist" aria-label="Sidebar panel">
    <button class="btn" role="tab" aria-selected={panel === "pages"} onclick={() => (panel = "pages")}>
      Pages
    </button>
    <button
      class="btn"
      role="tab"
      aria-selected={panel === "outline"}
      onclick={() => (panel = "outline")}
    >
      Outline
    </button>
  </div>
  {#if panel === "pages"}
    <div class="thumbs" bind:this={list}>
      {#each s.pages as _, i}
        <button class="thumb" class:current={i === s.page} data-page={i} onclick={() => s.goTo(i)} aria-label="Page {i + 1}">
          <span class="paper" class:invert={s.invert} style:width="{THUMB}px" style:height="{thumbH(i)}px">
            <img src={pageUrl(s.doc, i, 320)} alt="" loading="lazy" draggable="false" />
          </span>
          <span class="num">{i + 1}</span>
        </button>
      {/each}
    </div>
  {:else if s.outline === null}
    <div class="outline">
      {#each [70, 55, 80, 60, 45] as w}<div class="skeleton" style:width="{w}%"></div>{/each}
    </div>
  {:else if s.outline.length === 0}
    <p class="empty muted">This document has no outline. Use Pages to jump around, or {kbd('F')} to find text.</p>
  {:else}
    <div class="outline">{@render tree(s.outline, 0)}</div>
  {/if}
</aside>

{#snippet tree(items: OutlineItem[], depth: number)}
  {#each items as item}
    <button
      class="entry"
      style:padding-left="{8 + depth * 12}px"
      disabled={item.page === null}
      onclick={() => item.page !== null && s.goTo(item.page)}
      title={item.title}
    >
      <span class="title">{item.title}</span>
      {#if item.page !== null}<span class="num">{item.page + 1}</span>{/if}
    </button>
    {#if item.children.length}{@render tree(item.children, depth + 1)}{/if}
  {/each}
{/snippet}

<style>
  .sidebar {
    width: 200px;
    flex: none;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border-right: 1px solid var(--gray-5);
    animation: slide 200ms var(--ease-out);
  }
  .switch {
    display: flex;
    gap: 4px;
    padding: 8px;
  }
  .switch .btn {
    flex: 1;
  }
  .switch .btn[aria-selected="true"] {
    color: var(--text);
    background: var(--gray-4);
  }
  .thumbs,
  .outline {
    flex: 1;
    overflow-y: auto;
    padding: 4px 8px 16px;
  }
  .thumbs {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }
  .thumb {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 8px;
    border-radius: var(--r-control);
  }
  .paper {
    display: block;
    background: #fff;
    border-radius: 2px;
    overflow: hidden;
    box-shadow: 0 1px 3px oklch(5% 0.01 65 / 0.4);
    outline: 2px solid transparent;
    outline-offset: 2px;
    content-visibility: auto;
  }
  .paper.invert {
    filter: invert(0.92) hue-rotate(180deg);
  }
  .paper img {
    width: 100%;
    height: 100%;
    display: block;
  }
  .thumb.current .paper {
    outline-color: var(--accent-9);
  }
  .num {
    font-size: var(--fs-1);
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }
  .thumb.current .num {
    color: var(--text);
  }
  .entry {
    width: 100%;
    display: flex;
    gap: 8px;
    align-items: baseline;
    padding: 5px 8px;
    border-radius: var(--r-control);
    text-align: left;
    color: var(--text-2);
  }
  .entry .title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .entry:disabled {
    opacity: 0.6;
  }
  .empty {
    padding: 8px 16px;
    font-size: var(--fs-1);
  }
  .skeleton {
    height: 12px;
    margin: 10px 8px;
    border-radius: 4px;
    background: var(--gray-4);
  }
  @media (hover: hover) and (pointer: fine) {
    .entry:hover,
    .thumb:hover {
      background: var(--gray-4);
      color: var(--text);
    }
  }
  @keyframes slide {
    from {
      transform: translateX(-16px);
      opacity: 0;
    }
  }
</style>
