<script lang="ts">
  import { untrack } from 'svelte';
  import Header from '../components/Header.svelte';
  import Stepper from '../components/Stepper.svelte';
  import { OfflineError } from '../lib/api';
  import { draft, draftFor, resetDraft } from '../lib/draft.svelte';
  import { searchRecipes } from '../lib/recipes';
  import { href, router } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';
  import type { ListSource, RecipeSummary } from '../lib/types';

  /** The list the recipes go on. */
  let { id }: { id: number } = $props();

  const UNDO_MS = 4000;

  let query = $state('');
  /** Taken off, but not sent yet: undo is still possible. */
  let removing = $state<ListSource | null>(null);
  let removeTimer: ReturnType<typeof setTimeout> | undefined;
  let removeError = $state<string | null>(null);
  let creating = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    const listId = id;
    untrack(() => {
      draftFor(listId);
      void store.refreshRecipes();
    });
  });

  // Leaving the screen sends a removal that's still waiting for undo.
  $effect(() => () => void commitRemoval());

  const list = $derived(store.list(id));
  const onThisList = $derived(list?.recipes.filter((r) => r.path !== removing?.path) ?? []);
  /** Already on the list: shown, so you don't add a recipe twice by accident. */
  const onList = $derived(new Set(list?.recipes.map((r) => r.path) ?? []));

  const picked = $derived(new Set(draft.picks.map((p) => p.path)));
  const results = $derived(searchRecipes(store.recipes, query));

  function remove(r: ListSource) {
    void commitRemoval();
    removeError = null;
    removing = r;
    removeTimer = setTimeout(() => void commitRemoval(), UNDO_MS);
  }

  function undoRemoval() {
    clearTimeout(removeTimer);
    removing = null;
  }

  async function commitRemoval() {
    clearTimeout(removeTimer);
    const r = removing;
    if (!r) return;
    removing = null;
    try {
      await store.removeRecipe(id, r.path);
    } catch (e) {
      removeError =
        e instanceof OfflineError
          ? `Couldn’t take ${r.title} off: removing recipes needs a connection to the server.`
          : e instanceof Error
            ? e.message
            : String(e);
    }
  }

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
      await store.addRecipes(
        id,
        draft.picks.map((p) => ({ path: p.path, servings: p.servings ?? undefined })),
      );
      resetDraft();
      router.back();
    } catch (e) {
      error =
        e instanceof OfflineError
          ? 'Adding recipes needs a connection to the server.'
          : e instanceof Error
            ? e.message
            : String(e);
    } finally {
      creating = false;
    }
  }
</script>

<Header title={list ? `Add to ${list.title}` : 'Add recipes'} back />

<main>
  {#if onThisList.length > 0 || removeError}
    <section class="on-list">
      <h2>On this list</h2>
      {#if removeError}<p class="error small" role="alert">{removeError}</p>{/if}
      {#if onThisList.length}
        <ul class="card">
          {#each onThisList as r (r.path)}
            <li>
              <a class="title" href={href({ name: 'recipe', path: r.path, servings: r.servings ?? undefined })}>{r.title}</a>
              <span class="small muted">{r.servings ? `${r.servings} servings` : 'as written'}</span>
              <button class="remove" aria-label="Take {r.title} off the list" onclick={() => remove(r)}>×</button>
            </li>
          {/each}
        </ul>
        <p class="small muted hint">Taking a recipe off removes what only it needed, and its share of the rest.</p>
      {/if}
    </section>
  {/if}

  {#if draft.picks.length > 0}
    <section class="picked">
      <h2>Adding</h2>
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
      {#if error}<p class="error small" role="alert">{error}</p>{/if}
      <button class="btn primary create" onclick={create} disabled={creating}>
        {creating
          ? 'Adding…'
          : `Add ${draft.picks.length} ${draft.picks.length === 1 ? 'recipe' : 'recipes'} to the list`}
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
                    {#if onList.has(r.path)}<span class="already">already on the list · </span>{/if}{r.servings
                      ? `serves ${r.servings}`
                      : 'no servings'}{r.tags.length ? ` · ${r.tags.join(', ')}` : ''}
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

{#if removing}
  <div class="toast" role="status">
    <span>Took {removing.title} off the list</span>
    <button onclick={undoRemoval}>Undo</button>
  </div>
{/if}

<style>
  .already {
    color: var(--accent);
    font-weight: 600;
  }
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
  .picked li,
  .on-list li {
    padding: 0.4rem 0.25rem 0.4rem 1rem;
    min-height: var(--tap);
  }
  .hint {
    margin: -0.25rem 0.25rem 0;
  }
  .toast {
    position: fixed;
    left: 50%;
    bottom: calc(1.5rem + env(safe-area-inset-bottom));
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 0.75rem;
    background: var(--text);
    color: var(--bg);
    padding: 0.4rem 0.4rem 0.4rem 1rem;
    border-radius: 999px;
    font-size: 0.9rem;
    z-index: 30;
    max-width: calc(100vw - 2rem);
  }
  .toast button {
    border: 0;
    background: none;
    color: var(--accent);
    font-weight: 700;
    min-height: 36px;
    padding: 0 0.6rem;
    filter: brightness(1.4);
  }
  .picked .title,
  .on-list .title {
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
