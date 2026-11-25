<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    onclose,
    children,
    actions,
  }: { title: string; onclose: () => void; children: Snippet; actions: Snippet } = $props();

  let dialog: HTMLDivElement;
  const opener = document.activeElement as HTMLElement | null;

  $effect(() => {
    const first = dialog.querySelector<HTMLElement>("[data-autofocus], input, button.primary, button");
    first?.focus();
    return () => opener?.focus?.();
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      onclose();
    } else if (e.key === "Tab") {
      const items = [...dialog.querySelectorAll<HTMLElement>("input, button, a[href]")];
      const first = items[0];
      const last = items[items.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="dialog-title" bind:this={dialog} {onkeydown} tabindex="-1">
    <h2 id="dialog-title">{title}</h2>
    <div class="body">{@render children()}</div>
    <div class="actions">{@render actions()}</div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    background: oklch(5% 0.01 65 / 0.5);
    animation: fade 150ms var(--ease-out);
  }
  .dialog {
    width: min(420px, calc(100vw - 32px));
    padding: 24px;
    border-radius: var(--r-float);
    background: var(--gray-3);
    border: 1px solid var(--gray-6);
    box-shadow: var(--shadow-float);
    animation: enter 200ms var(--ease-out);
    outline: none;
  }
  h2 {
    margin: 0 0 8px;
    font-size: var(--fs-3);
    font-weight: 500;
  }
  .body {
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .actions {
    margin-top: 24px;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes enter {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
  }
</style>
