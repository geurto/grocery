<script lang="ts">
  import type { Snippet } from 'svelte';
  import { router } from '../lib/router.svelte';
  import Status from './Status.svelte';

  let {
    title,
    back = false,
    actions,
  }: { title: string; back?: boolean; actions?: Snippet } = $props();
</script>

<header>
  <div class="bar">
    {#if back}
      <button class="icon" onclick={() => router.back()} aria-label="Back">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 5l-7 7 7 7" /></svg>
      </button>
    {/if}
    <h1>{title}</h1>
    <Status />
    {@render actions?.()}
  </div>
</header>

<style>
  header {
    position: sticky;
    top: 0;
    z-index: 10;
    background: var(--bg);
    border-bottom: 1px solid var(--line);
    padding-top: env(safe-area-inset-top);
  }
  .bar {
    max-width: 40rem;
    margin: 0 auto;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-height: 56px;
    padding: 0 max(0.5rem, env(safe-area-inset-left));
  }
  h1 {
    flex: 1;
    font-size: 1.15rem;
    padding-left: 0.5rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .icon {
    width: 44px;
    height: 44px;
    border: 0;
    background: none;
    border-radius: 50%;
    display: grid;
    place-items: center;
  }
  svg {
    width: 24px;
    height: 24px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
