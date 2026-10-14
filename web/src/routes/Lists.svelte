<script lang="ts">
  import { untrack } from 'svelte';
  import Header from '../components/Header.svelte';
  import { href } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';
  import type { ListSummary } from '../lib/types';

  // On open; live updates keep it current after that.
  $effect(() => {
    untrack(() => void store.refreshLists());
  });

  const open = $derived(store.summaries.filter((l) => l.status === 'open'));
  const done = $derived(store.summaries.filter((l) => l.status === 'done'));

  function date(created: string) {
    const d = new Date(created);
    return isNaN(d.getTime())
      ? ''
      : d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  }
</script>

{#snippet row(list: ListSummary)}
  {@const pct = list.item_count ? (list.checked_count / list.item_count) * 100 : 0}
  <li>
    <a class="card list" href={href({ name: 'list', id: list.id })}>
      <span class="title">{list.title}</span>
      <span class="meta muted small">
        {date(list.created)}
        {#if store.pending(list.id) > 0}· <span class="sync">{store.pending(list.id)} to sync</span>{/if}
      </span>
      <span class="progress" aria-label="{list.checked_count} of {list.item_count} done">
        <span class="bar"><span style:width="{pct}%"></span></span>
        <span class="count small">{list.checked_count}/{list.item_count}</span>
      </span>
    </a>
  </li>
{/snippet}

<Header title="Shopping lists" />

<main>
  {#if open.length === 0 && done.length === 0}
    <div class="empty">
      <p>No lists yet.</p>
      <p class="small">Pick a few recipes and the list puts itself together, sorted by aisle.</p>
    </div>
  {/if}

  {#if open.length > 0}
    <ul>
      {#each open as list (list.id)}
        {@render row(list)}
      {/each}
    </ul>
  {/if}

  {#if done.length > 0}
    <details>
      <summary class="muted">Done ({done.length})</summary>
      <ul class="done">
        {#each done as list (list.id)}
          {@render row(list)}
        {/each}
      </ul>
    </details>
  {/if}
</main>

<a class="btn primary fab" href={href({ name: 'new' })}>
  <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
  New list
</a>

<style>
  ul {
    list-style: none;
    padding: 0;
    margin: 0.25rem 0 1rem;
    display: grid;
    gap: 0.6rem;
  }
  .list {
    display: grid;
    gap: 0.3rem;
    padding: 0.9rem 1rem;
    text-decoration: none;
    color: inherit;
  }
  .title {
    font-weight: 650;
    font-size: 1.05rem;
  }
  .sync {
    color: var(--warn);
  }
  .progress {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  .bar {
    flex: 1;
    height: 6px;
    border-radius: 3px;
    background: var(--surface-2);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: 3px;
    transition: width 0.3s;
  }
  .count {
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  details summary {
    padding: 0.75rem 0.25rem;
    cursor: pointer;
  }
  .done .list {
    opacity: 0.7;
  }
  .fab {
    position: fixed;
    right: max(1rem, env(safe-area-inset-right));
    bottom: calc(1rem + var(--tabbar) + env(safe-area-inset-bottom));
    min-height: 54px;
    padding: 0 1.3rem;
    box-shadow: 0 6px 20px rgb(0 0 0 / 0.18);
  }
  .fab svg {
    width: 22px;
    height: 22px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.4;
    stroke-linecap: round;
  }
</style>
