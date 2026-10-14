<script lang="ts">
  import { untrack } from 'svelte';
  import Header from '../components/Header.svelte';
  import { longDay } from '../lib/dates';
  import { MEALS, planner } from '../lib/planner.svelte';
  import { byFolder, searchRecipes } from '../lib/recipes';
  import { router } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';
  import type { Meal, RecipeSummary } from '../lib/types';

  let { day, meal }: { day: string; meal: Meal | null } = $props();

  let query = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    untrack(() => void store.refreshRecipes());
  });

  const groups = $derived(byFolder(searchRecipes(store.recipes, query)));
  const mealLabel = $derived(MEALS.find((m) => m.meal === meal)?.label);

  async function pick(r: RecipeSummary) {
    const person = planner.person;
    if (!person || busy) return;
    busy = true;
    error = null;
    try {
      await planner.add(person.id, day, meal, r.path);
      router.back();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<Header title={mealLabel ? `Add to ${mealLabel.toLowerCase()}` : 'Add to plan'} back />

<main>
  <p class="muted small for">{planner.person?.name} · {longDay(day)} · one portion; change it on the plan.</p>
  <input class="field search" type="search" bind:value={query} placeholder="Search title, tag or ingredient" aria-label="Search recipes" />
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#each groups as group (group.folder)}
    <section>
      {#if group.folder}<h2>{group.folder}</h2>{/if}
      <ul class="card">
        {#each group.recipes as r (r.path)}
          <li>
            <button onclick={() => pick(r)} disabled={busy || !!r.error}>
              <span class="title">{r.title}</span>
              <span class="small muted">{r.servings ? `serves ${r.servings}` : 'no servings: can’t count a portion'}</span>
            </button>
          </li>
        {/each}
      </ul>
    </section>
  {/each}
</main>

<style>
  .for {
    margin: 0 0.25rem 0.75rem;
  }
  .search {
    margin-bottom: 1rem;
  }
  .error {
    color: var(--danger);
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
  button {
    width: 100%;
    display: grid;
    gap: 0.1rem;
    min-height: var(--tap);
    padding: 0.6rem 1rem;
    border: 0;
    background: none;
    text-align: left;
  }
  .title {
    font-weight: 600;
  }
</style>
