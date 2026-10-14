<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    onclose,
    children,
    footer,
  }: { title: string; onclose: () => void; children: Snippet; footer?: Snippet } = $props();

  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose();
  }
</script>

<svelte:window onkeydown={key} />

<div class="backdrop" role="presentation" onclick={onclose}></div>
<div class="sheet" role="dialog" aria-modal="true" aria-label={title}>
  <header>
    <h2>{title}</h2>
    <button class="close" onclick={onclose} aria-label="Close">×</button>
  </header>
  <div class="content">{@render children()}</div>
  {#if footer}<footer>{@render footer()}</footer>{/if}
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: rgb(0 0 0 / 0.4);
  }
  .sheet {
    position: fixed;
    z-index: 51;
    left: 0;
    right: 0;
    bottom: 0;
    max-width: 40rem;
    margin: 0 auto;
    max-height: 92dvh;
    display: flex;
    flex-direction: column;
    background: var(--bg);
    border-radius: 20px 20px 0 0;
    border: 1px solid var(--line);
    box-shadow: 0 -8px 30px rgb(0 0 0 / 0.2);
    padding-bottom: env(safe-area-inset-bottom);
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.5rem 0.4rem 1.1rem;
    border-bottom: 1px solid var(--line);
  }
  h2 {
    flex: 1;
    font-size: 1.1rem;
  }
  .close {
    width: 44px;
    height: 44px;
    border: 0;
    background: none;
    font-size: 1.6rem;
    line-height: 1;
    color: var(--muted);
  }
  .content {
    overflow-y: auto;
    padding: 1rem 1.1rem;
    overscroll-behavior: contain;
  }
  footer {
    padding: 0.6rem 1.1rem 0.8rem;
    border-top: 1px solid var(--line);
    display: flex;
    gap: 0.5rem;
  }
  footer :global(.btn) {
    flex: 1;
    min-height: 48px;
  }
</style>
