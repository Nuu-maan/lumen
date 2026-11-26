<script lang="ts">
  import type { ArchiveEntry } from "../lib/api";
  import { formatSize } from "../lib/format";
  import Icon from "../components/Icon.svelte";

  let { entries, onopen }: { entries: ArchiveEntry[]; onopen: (index: number) => Promise<void> } = $props();
  let filter = $state("");
  let opening = $state<number | null>(null);
  let failed = $state<string | null>(null);

  const files = $derived(entries.filter((e) => !e.dir));
  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const list = q ? files.filter((e) => e.path.toLowerCase().includes(q)) : files;
    return list.slice().sort((a, b) => a.path.localeCompare(b.path));
  });
  const totalSize = $derived(files.reduce((sum, e) => sum + e.size, 0));

  async function open(e: ArchiveEntry) {
    opening = e.index;
    failed = null;
    try {
      await onopen(e.index);
    } catch (err) {
      failed = `${e.path}: ${err}`;
    } finally {
      opening = null;
    }
  }

  function split(path: string) {
    const i = path.lastIndexOf("/");
    return i < 0 ? ["", path] : [path.slice(0, i + 1), path.slice(i + 1)];
  }
</script>

<div class="archive">
  <div class="bar">
    <input class="field" bind:value={filter} placeholder="Filter files" aria-label="Filter files" spellcheck="false" />
    <span class="muted">{files.length} files, {formatSize(totalSize)} uncompressed</span>
  </div>
  {#if failed}<p class="error" role="alert">Couldn't open {failed}</p>{/if}
  {#if files.length === 0}
    <p class="empty muted">This archive is empty.</p>
  {:else if shown.length === 0}
    <p class="empty muted">No files match “{filter}”.</p>
  {:else}
    <ul class="list" aria-label="Archive contents">
      {#each shown as e (e.index)}
        {@const [dir, name] = split(e.path)}
        <li>
          <button class="entry" ondblclick={() => open(e)} onkeydown={(k) => k.key === "Enter" && open(e)} title="Double-click or press Enter to open">
            <Icon name="file" size={14} />
            <span class="path"><span class="muted">{dir}</span>{name}</span>
            <span class="size muted">{opening === e.index ? "Opening…" : formatSize(e.size)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .archive {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--gray-5);
  }
  .bar .field {
    width: 280px;
  }
  .list {
    flex: 1;
    overflow: auto;
    list-style: none;
    margin: 0;
    padding: 4px 8px;
  }
  .entry {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 8px;
    border-radius: var(--r-control);
    text-align: left;
    color: var(--text-2);
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }
  .size {
    font-variant-numeric: tabular-nums;
    font-size: var(--fs-1);
  }
  .empty,
  .error {
    padding: 16px;
    margin: 0;
  }
  .error {
    color: var(--red-11);
  }
  @media (hover: hover) and (pointer: fine) {
    .entry:hover {
      background: var(--gray-3);
    }
  }
</style>
