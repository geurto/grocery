<script lang="ts">
  import { untrack } from 'svelte';
  import AmountSheet from '../components/AmountSheet.svelte';
  import Header from '../components/Header.svelte';
  import { longDay } from '../lib/dates';
  import { amountText, parseTyped, suggestFoods, type Suggestion } from '../lib/foods';
  import { MEALS, planner } from '../lib/planner.svelte';
  import { byFolder, searchRecipes } from '../lib/recipes';
  import { router } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';
  import type { Meal, RecipeSummary } from '../lib/types';

  let { day, meal }: { day: string; meal: Meal | null } = $props();

  let query = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);

  /** A food picked, to say how much of. */
  let picked = $state<Suggestion | null>(null);

  $effect(() => {
    untrack(() => {
      void store.refreshRecipes();
      void store.refreshFoods();
    });
  });

  const groups = $derived(byFolder(searchRecipes(store.recipes, query)));
  // "25 g cashews" searches for cashews, and keeps the 25 g for the sheet.
  const typed = $derived(parseTyped(query));
  const foods = $derived(suggestFoods(store.foods, typed.name, 4));
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

  async function addFood(name: string, amount: number, unit: string) {
    const person = planner.person;
    if (!person) return;
    error = null;
    try {
      await planner.addFood(person.id, day, meal, name, amount, unit);
      router.back();
    } catch (e) {
      picked = null;
      error = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<Header title={mealLabel ? `Add to ${mealLabel.toLowerCase()}` : 'Add to plan'} back />

<main>
  <p class="muted small for">{planner.person?.name} · {longDay(day)} · a recipe is one portion; change it on the plan.</p>
  <input
    class="field search"
    type="search"
    bind:value={query}
    placeholder="A recipe, or a food (“25 g cashews”)"
    aria-label="Search recipes and foods"
  />
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if foods.length}
    <section>
      <h2>Foods</h2>
      <ul class="card">
        {#each foods as s (s.food.name)}
          <li>
            <button onclick={() => (picked = s)} disabled={busy}>
              <span class="title">{s.matched}{#if s.matched !== s.food.name}<span class="muted"> ({s.food.name})</span>{/if}</span>
              <span class="small muted">
                {typed.amount !== undefined ? `${amountText(typed.amount, typed.unit)} · ` : ''}single food
              </span>
            </button>
          </li>
        {/each}
      </ul>
    </section>
    {#if groups.length}<h2 class="recipes">Recipes</h2>{/if}
  {/if}
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

{#if picked}
  <AmountSheet
    title="How much?"
    name={picked.matched}
    food={picked.food}
    amount={typed.amount}
    unit={typed.amount !== undefined ? (typed.unit ?? '') : undefined}
    onsave={(amount, unit) => addFood(picked!.matched, amount, unit)}
    onclose={() => (picked = null)}
  />
{/if}

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
  .recipes {
    margin-top: 0.5rem;
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
