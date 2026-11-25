<script lang="ts">
  import type { Email } from "../lib/api";
  import Icon from "../components/Icon.svelte";
  import { formatSize } from "../lib/format";

  let { email, onopen }: { email: Email; onopen: (index: number) => Promise<void> } = $props();
  let opening = $state<number | null>(null);
  let failed = $state<string | null>(null);

  const date = $derived(email.date ? new Date(email.date).toLocaleString(undefined, { dateStyle: "full", timeStyle: "short" }) : "");

  async function open(index: number) {
    opening = index;
    failed = null;
    try {
      await onopen(index);
    } catch (e) {
      failed = String(e);
    } finally {
      opening = null;
    }
  }
</script>

<div class="email">
  <header>
    <h1>{email.subject || "(no subject)"}</h1>
    <dl>
      {#if email.from}<dt>From</dt><dd>{email.from}</dd>{/if}
      {#if email.to}<dt>To</dt><dd>{email.to}</dd>{/if}
      {#if email.cc}<dt>Cc</dt><dd>{email.cc}</dd>{/if}
      {#if date}<dt>Date</dt><dd>{date}</dd>{/if}
    </dl>
    {#if email.attachments.length}
      <ul class="attachments" aria-label="Attachments">
        {#each email.attachments as a}
          <li>
            <button class="chip" onclick={() => open(a.index)} disabled={opening === a.index} title="Open {a.name} in a new tab">
              <Icon name="paperclip" size={14} />
              <span class="name">{a.name}</span>
              <span class="muted">{formatSize(a.size)}</span>
            </button>
          </li>
        {/each}
      </ul>
      {#if failed}<p class="error" role="alert">Couldn't open the attachment: {failed}</p>{/if}
    {/if}
  </header>
  <iframe class="body" sandbox="" srcdoc={email.html} title="Message body" referrerpolicy="no-referrer"></iframe>
</div>

<style>
  .email {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
  }
  header {
    padding: 16px 24px;
    border-bottom: 1px solid var(--gray-5);
    background: var(--surface);
  }
  h1 {
    margin: 0 0 8px;
    font-size: var(--fs-4);
    font-weight: 600;
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 2px 12px;
    margin: 0;
  }
  dt {
    color: var(--gray-8);
  }
  dd {
    margin: 0;
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .attachments {
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 12px 0 0;
    padding: 0;
  }
  .chip {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 320px;
    height: 32px;
    padding: 0 12px;
    border-radius: var(--r-control);
    background: var(--gray-3);
    border: 1px solid var(--gray-6);
    transition: background-color 120ms ease, transform 120ms var(--ease-out);
  }
  .chip:active {
    transform: scale(0.97);
  }
  .chip:disabled {
    opacity: 0.6;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .error {
    margin: 8px 0 0;
    color: var(--red-11);
  }
  .body {
    flex: 1;
    border: 0;
    background: #fff;
  }
  @media (hover: hover) and (pointer: fine) {
    .chip:hover {
      background: var(--gray-4);
    }
  }
</style>
