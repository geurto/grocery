<script lang="ts">
  import { tick, untrack } from 'svelte';
  import Header from '../components/Header.svelte';
  import RecipePrint from '../components/RecipePrint.svelte';
  import { exportTitle, inListOrder } from '../lib/export';
  import { byFolder, searchRecipes } from '../lib/recipes';
  import { href } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';
  import type { Recipe, RecipeSummary } from '../lib/types';

  let query = $state('');
  /** Picking recipes to export, instead of opening them. */
  let selecting = $state(false);
  let picked = $state(new Set<string>());
  let exporting = $state(false);
  /** Laid out for print while the print dialog is open. */
  let printing = $state<Recipe[]>([]);
  let toast = $state<string | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    untrack(() => void store.refreshRecipes());
  });

  const results = $derived(searchRecipes(store.recipes, query));
  const groups = $derived(byFolder(results));
  /** What "All" picks: the recipes shown that can be exported. */
  const pickable = $derived(results.filter((r) => !r.error));
  const allPicked = $derived(pickable.length > 0 && pickable.every((r) => picked.has(r.path)));

  function say(message: string) {
    clearTimeout(toastTimer);
    toast = message;
    toastTimer = setTimeout(() => (toast = null), 4000);
  }

  function toggle(r: RecipeSummary) {
    const next = new Set(picked);
    if (!next.delete(r.path)) next.add(r.path);
    picked = next;
  }

  function toggleAll() {
    const next = new Set(picked);
    for (const r of pickable) {
      if (allPicked) next.delete(r.path);
      else next.add(r.path);
    }
    picked = next;
  }

  function stopSelecting() {
    selecting = false;
    picked = new Set();
  }

  async function exportPdf() {
    if (exporting || picked.size === 0) return;
    exporting = true;
    try {
      // In the order they're listed, whatever order they were tapped in.
      const chosen = inListOrder(
        byFolder(store.recipes).flatMap((g) => g.recipes),
        picked,
      );
      // From the server, or the phone's cache when offline.
      const loaded = await Promise.all(chosen.map((r) => store.recipe(r.path).catch(() => undefined)));
      const ready = loaded.filter((r): r is Recipe => !!r?.display && !r.error);
      const missing = chosen.filter((r) => !ready.some((x) => x.path === r.path)).map((r) => r.title);
      if (ready.length === 0) {
        say('These recipes aren’t on this phone yet, and the server can’t be reached.');
        return;
      }
      printing = ready;
      await tick();
      // Browsers offer the title as the PDF's file name.
      const title = document.title;
      document.title = exportTitle(ready.map((r) => r.title));
      addEventListener(
        'afterprint',
        () => {
          document.title = title;
          printing = [];
        },
        { once: true },
      );
      // The print dialog, with "Save as PDF" as a destination.
      window.print();
      if (missing.length) say(`Left out, not on this phone yet: ${missing.join(', ')}`);
    } finally {
      exporting = false;
    }
  }
</script>

<Header title={selecting ? (picked.size ? `${picked.size} selected` : 'Select recipes') : 'Recipes'}>
  {#snippet actions()}
    {#if selecting}
      <button class="btn head-btn" onclick={toggleAll} disabled={pickable.length === 0}>
        {allPicked ? 'None' : 'All'}
      </button>
      <button class="btn head-btn" onclick={stopSelecting}>Done</button>
    {:else}
      <button class="btn head-btn" onclick={() => (selecting = true)} disabled={store.recipes.length === 0}>
        Select
      </button>
      <a class="btn head-btn" href={href({ name: 'foods' })}>Foods</a>
    {/if}
  {/snippet}
</Header>

<main>
  <input
    class="field search"
    type="search"
    bind:value={query}
    placeholder="Search title, tag or ingredient"
    aria-label="Search recipes"
  />
  {#if store.recipes.length === 0}
    <p class="empty">{store.online ? 'No recipes found in Recipes/.' : 'Recipes load when you’re online.'}</p>
  {:else if results.length === 0}
    <p class="empty">Nothing matches “{query}”.</p>
  {:else}
    {#each groups as group (group.folder)}
      <section>
        {#if group.folder}<h2>{group.folder}</h2>{/if}
        <ul class="card">
          {#each group.recipes as r (r.path)}
            {#snippet info()}
              <span class="info">
                <span class="title">{r.title}</span>
                {#if r.error}
                  <span class="small error">Doesn’t parse; fix the .cook file</span>
                {:else}
                  <span class="small muted">
                    {r.servings ? `serves ${r.servings}` : 'no servings'}{r.tags.length ? ` · ${r.tags.join(', ')}` : ''}
                  </span>
                {/if}
              </span>
            {/snippet}
            <li class:on={picked.has(r.path)}>
              {#if selecting}
                <button
                  class="row pick"
                  onclick={() => toggle(r)}
                  disabled={!!r.error}
                  aria-pressed={picked.has(r.path)}
                >
                  <span class="check" aria-hidden="true">{picked.has(r.path) ? '✓' : ''}</span>
                  {@render info()}
                </button>
              {:else}
                <a class="row" href={href({ name: 'recipe', path: r.path })}>{@render info()}</a>
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/each}
  {/if}
</main>

{#if selecting}
  <button class="btn primary fab" onclick={exportPdf} disabled={picked.size === 0 || exporting}>
    {exporting ? 'Preparing…' : picked.size ? `Export ${picked.size} to PDF` : 'Export to PDF'}
  </button>
{:else}
  <a class="btn primary fab" href={href({ name: 'recipe-new' })}>+ New recipe</a>
{/if}

{#if toast}
  <div class="toast" role="status">{toast}</div>
{/if}

{#if printing.length}
  <RecipePrint recipes={printing} />
{/if}

<style>
  .head-btn {
    min-height: 38px;
    padding: 0 0.9rem;
    font-size: 0.9rem;
  }
  .fab {
    position: fixed;
    right: max(1rem, env(safe-area-inset-right));
    bottom: calc(1rem + var(--tabbar) + env(safe-area-inset-bottom));
    min-height: 54px;
    padding: 0 1.3rem;
    box-shadow: 0 6px 20px rgb(0 0 0 / 0.18);
  }
  .search {
    margin-bottom: 1rem;
  }
  section {
    margin-bottom: 1.25rem;
  }
  h2 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 0 0 0.5rem 0.25rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: hidden;
  }
  li + li {
    border-top: 1px solid var(--line);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    width: 100%;
    min-height: var(--tap);
    padding: 0.6rem 1rem;
    color: inherit;
    text-decoration: none;
  }
  .pick {
    border: 0;
    background: none;
    text-align: left;
  }
  .pick:disabled {
    opacity: 0.55;
  }
  .info {
    display: grid;
    gap: 0.1rem;
  }
  .check {
    flex: none;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    border: 2px solid var(--line);
    font-weight: 700;
    color: var(--accent-text);
  }
  .on .check {
    background: var(--accent);
    border-color: var(--accent);
  }
  .title {
    font-weight: 600;
  }
  .error {
    color: var(--danger);
  }
  .toast {
    position: fixed;
    left: 50%;
    bottom: calc(5.5rem + var(--tabbar) + env(safe-area-inset-bottom));
    transform: translateX(-50%);
    width: max-content;
    max-width: calc(100vw - 2rem);
    background: var(--text);
    color: var(--bg);
    padding: 0.6rem 1rem;
    border-radius: 14px;
    font-size: 0.9rem;
    z-index: 30;
  }
</style>
