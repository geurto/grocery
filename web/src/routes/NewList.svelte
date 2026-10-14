<script lang="ts">
  import { untrack } from 'svelte';
  import Header from '../components/Header.svelte';
  import Stepper from '../components/Stepper.svelte';
  import { OfflineError } from '../lib/api';
  import { draft, resetDraft } from '../lib/draft.svelte';
  import { href, router } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';
  import type { RecipeSummary } from '../lib/types';

  let query = $state('');
  let creating = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    untrack(() => void store.refreshRecipes());
  });

  const picked = $derived(new Set(draft.picks.map((p) => p.path)));
  const results = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return store.recipes;
    return store.recipes.filter(
      (r) =>
        r.title.toLowerCase().includes(q) ||
        r.tags.some((t) => t.toLowerCase().includes(q)) ||
        r.ingredients.some((i) => i.toLowerCase().includes(q)),
    );
  });

  function toggle(r: RecipeSummary) {
    if (picked.has(r.path)) {
      draft.picks = draft.picks.filter((p) => p.path !== r.path);
    } else {
      draft.picks.push({ path: r.path, title: r.title, servings: r.servings });
    }
  }

  async function create() {
    creating = true;
    error = null;
    try {
      const list = await store.createList(
        draft.picks.map((p) => ({ path: p.path, servings: p.servings ?? undefined })),
        draft.title.trim() || undefined,
      );
      resetDraft();
      router.go({ name: 'list', id: list.id }, true);
    } catch (e) {
      error =
        e instanceof OfflineError
          ? 'Making a list needs a connection to the server.'
          : e instanceof Error
            ? e.message
            : String(e);
    } finally {
      creating = false;
    }
  }
</script>

<Header title="New list" back />

<main>
  {#if draft.picks.length > 0}
    <section class="picked">
      <h2>On this list</h2>
      <ul class="card">
        {#each draft.picks as pick (pick.path)}
          <li>
            <a class="title" href={href({ name: 'recipe', path: pick.path, servings: pick.servings ?? undefined })}
              >{pick.title}</a
            >
            {#if pick.servings !== null}
              <Stepper bind:value={pick.servings} label="servings" />
            {:else}
              <span class="small muted">as written</span>
            {/if}
            <button
              class="remove"
              aria-label="Remove {pick.title}"
              onclick={() => (draft.picks = draft.picks.filter((p) => p.path !== pick.path))}>×</button
            >
          </li>
        {/each}
      </ul>
      <input class="field" bind:value={draft.title} placeholder="Title (optional)" aria-label="List title" />
      {#if error}<p class="error small" role="alert">{error}</p>{/if}
      <button class="btn primary create" onclick={create} disabled={creating}>
        {creating ? 'Making the list…' : `Make shopping list (${draft.picks.length})`}
      </button>
    </section>
  {/if}

  <section>
    <h2>Recipes</h2>
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
      <ul class="card results">
        {#each results as r (r.path)}
          <li class:on={picked.has(r.path)}>
            <button class="pick" onclick={() => toggle(r)} disabled={!!r.error} aria-pressed={picked.has(r.path)}>
              <span class="check" aria-hidden="true">{picked.has(r.path) ? '✓' : '+'}</span>
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
            </button>
            {#if !r.error}
              <a class="view small" href={href({ name: 'recipe', path: r.path })}>View</a>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</main>

<style>
  section {
    margin-bottom: 1.5rem;
  }
  h2 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 0.25rem 0 0.5rem 0.25rem;
  }
  ul {
    list-style: none;
    margin: 0 0 0.75rem;
    padding: 0;
    overflow: hidden;
  }
  li {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    border-top: 1px solid var(--line);
  }
  li:first-child {
    border-top: 0;
  }
  .picked li {
    padding: 0.4rem 0.25rem 0.4rem 1rem;
    min-height: var(--tap);
  }
  .picked .title {
    flex: 1;
    font-weight: 600;
    color: inherit;
    text-decoration: none;
  }
  .remove {
    width: 44px;
    height: 44px;
    border: 0;
    background: none;
    font-size: 1.4rem;
    color: var(--muted);
  }
  .create {
    width: 100%;
    min-height: 52px;
    margin-top: 0.75rem;
  }
  .search {
    margin-bottom: 0.75rem;
  }
  .pick {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.8rem;
    min-height: var(--tap);
    padding: 0.55rem 0 0.55rem 0.9rem;
    border: 0;
    background: none;
    text-align: left;
  }
  .check {
    flex: none;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    border: 2px solid var(--line);
    font-weight: 700;
    color: var(--muted);
  }
  .on .check {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  .info {
    display: grid;
  }
  .info .title {
    font-weight: 600;
  }
  .view {
    padding: 0.75rem 1rem;
  }
  .error {
    color: var(--danger);
  }
</style>
