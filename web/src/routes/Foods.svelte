<script lang="ts">
  import { untrack } from 'svelte';
  import FoodSheet from '../components/FoodSheet.svelte';
  import Header from '../components/Header.svelte';
  import { api, OfflineError } from '../lib/api';
  import type { Food } from '../lib/types';

  let foods = $state<Food[] | null>(null);
  let error = $state<string | null>(null);
  let query = $state('');
  let adding = $state(false);

  async function load() {
    try {
      foods = await api.foods();
      error = null;
    } catch (e) {
      error =
        e instanceof OfflineError
          ? 'The food list needs a connection to the server.'
          : e instanceof Error
            ? e.message
            : String(e);
    }
  }

  $effect(() => {
    untrack(() => void load());
  });

  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!foods) return [];
    if (!q) return foods;
    return foods.filter(
      (f) =>
        f.name.toLowerCase().includes(q) ||
        f.synonyms.some((s) => s.toLowerCase().includes(q)) ||
        (f.aisle ?? '').toLowerCase().includes(q),
    );
  });

  function source(f: Food): string {
    if (f.negligible) return 'negligible';
    if (f.nevo !== null) return 'NEVO';
    if (f.custom !== null) return 'own values';
    return 'no nutrition';
  }
</script>

<Header title="Foods" back />

<main>
  <input class="field search" type="search" bind:value={query} placeholder="Search name, other name or aisle" aria-label="Search foods" />
  {#if error}
    <p class="empty">{error}</p>
  {:else if foods === null}
    <p class="empty">Loading…</p>
  {:else if shown.length === 0}
    <p class="empty">{foods.length ? `Nothing matches “${query}”.` : 'No foods yet.'}</p>
  {:else}
    <ul class="card">
      {#each shown as f (f.name)}
        <li>
          <span class="name">{f.name}</span>
          <span class="small muted">
            {source(f)}{f.aisle ? ` · ${f.aisle}` : ''}{f.synonyms.length ? ` · ${f.synonyms.join(', ')}` : ''}
          </span>
        </li>
      {/each}
    </ul>
    <p class="small muted count">{shown.length} of {foods.length}. To change a food, edit Food/aliases.yaml.</p>
  {/if}
</main>

<button class="btn primary fab" onclick={() => (adding = true)}>+ Add food</button>

{#if adding}
  <FoodSheet
    name={query.trim()}
    onsaved={() => {
      adding = false;
      void load();
    }}
    onclose={() => (adding = false)}
  />
{/if}

<style>
  .search {
    margin-bottom: 0.75rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: grid;
    gap: 0.1rem;
    padding: 0.6rem 1rem;
  }
  li + li {
    border-top: 1px solid var(--line);
  }
  .name {
    font-weight: 600;
  }
  .count {
    margin: 0.6rem 0.25rem;
  }
  .fab {
    position: fixed;
    right: max(1rem, env(safe-area-inset-right));
    bottom: calc(1rem + env(safe-area-inset-bottom));
    min-height: 54px;
    padding: 0 1.3rem;
    box-shadow: 0 6px 20px rgb(0 0 0 / 0.18);
  }
</style>
