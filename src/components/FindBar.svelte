<script lang="ts">
  import { untrack } from "svelte";
  import Icon from "./Icon.svelte";

  let {
    query,
    count,
    index,
    searching,
    onsearch,
    onnext,
    onclose,
  }: {
    query: string;
    count: number;
    index: number;
    searching: boolean;
    onsearch: (q: string) => void;
    onnext: (dir: 1 | -1) => void;
    onclose: () => void;
  } = $props();

  let input: HTMLInputElement;
  let value = $state(untrack(() => query));
  let timer = 0;

  export function focus() {
    input.focus();
    input.select();
  }

  $effect(() => focus());

  function oninput() {
    clearTimeout(timer);
    timer = window.setTimeout(() => onsearch(value), 150);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      if (value !== query) {
        clearTimeout(timer);
        onsearch(value);
      } else onnext(e.shiftKey ? -1 : 1);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }

  const status = $derived(
    !query.trim() ? "" : count ? `${index + 1} of ${count}${searching ? "+" : ""}` : searching ? "Searching…" : "No matches",
  );
</script>

<div class="find" role="search">
  <input
    class="field"
    bind:this={input}
    bind:value
    {oninput}
    {onkeydown}
    placeholder="Find in document"
    aria-label="Find in document"
    spellcheck="false"
  />
  <span class="status muted" aria-live="polite">{status}</span>
  <button class="btn" title="Previous match (Shift+Enter)" aria-label="Previous match" disabled={!count} onclick={() => onnext(-1)}>
    <Icon name="up" />
  </button>
  <button class="btn" title="Next match (Enter)" aria-label="Next match" disabled={!count} onclick={() => onnext(1)}>
    <Icon name="down" />
  </button>
  <button class="btn" title="Close (Esc)" aria-label="Close find" onclick={onclose}><Icon name="close" /></button>
</div>

<style>
  .find {
    position: absolute;
    top: 8px;
    right: 20px;
    z-index: 10;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px;
    border-radius: var(--r-float);
    background: var(--gray-3);
    border: 1px solid var(--gray-6);
    box-shadow: var(--shadow-float);
    animation: drop 180ms var(--ease-out);
  }
  .field {
    width: 220px;
  }
  .status {
    min-width: 76px;
    padding: 0 4px;
    font-size: var(--fs-1);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-6px);
    }
  }
</style>
