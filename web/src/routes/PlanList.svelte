<script lang="ts">
  import { untrack } from 'svelte';
  import Header from '../components/Header.svelte';
  import { parseDay, today, weekDays, weekLabel } from '../lib/dates';
  import { plannedRecipes } from '../lib/plan';
  import { planner } from '../lib/planner.svelte';
  import { router } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';

  let { start }: { start: string } = $props();

  const days = $derived(weekDays(start));
  // From today on when this week is the current one, else the whole week.
  let chosenDays = $state(
    untrack(() => {
      const all = weekDays(start);
      const t = today();
      return new Set(all.includes(t) ? all.filter((d) => d >= t) : all);
    }),
  );
  let chosenPeople = $state(new Set(untrack(() => planner.people.map((p) => p.id))));
  let creating = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    const s = start;
    untrack(() => {
      void store.refreshRecipes();
      for (const p of planner.people) void planner.refreshWeek(p.id, s);
    });
  });

  const entries = $derived(
    planner.people.filter((p) => chosenPeople.has(p.id)).flatMap((p) => planner.entries(p.id, start) ?? []),
  );
  const planned = $derived(plannedRecipes(entries, chosenDays));
  const servingsOf = (path: string) => store.recipes.find((r) => r.path === path)?.servings ?? null;
  const name = (id: number) => planner.people.find((p) => p.id === id)?.name ?? '?';
  const servingsText = (n: number) => `${fmt(n)} ${n === 1 ? 'serving' : 'servings'}`;
  const fmt = (n: number) => (Number.isInteger(n) ? String(n) : n.toFixed(1).replace(/\.0$/, ''));

  function toggle<T>(set: Set<T>, v: T): Set<T> {
    const next = new Set(set);
    if (!next.delete(v)) next.add(v);
    return next;
  }

  async function create() {
    creating = true;
    error = null;
    try {
      const list = await store.createList(
        planned.map((r) => ({
          path: r.path,
          // A recipe without servings can't be scaled: once, as written.
          servings: servingsOf(r.path) ? r.portions : undefined,
        })),
        `Plan ${weekLabel(start)}`,
      );
      router.go({ name: 'list', id: list.id });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      creating = false;
    }
  }
</script>

<Header title="List from plan" back />

<main>
  <section>
    <h2>Days · {weekLabel(start)}</h2>
    <div class="chips">
      {#each days as d (d)}
        <button aria-pressed={chosenDays.has(d)} onclick={() => (chosenDays = toggle(chosenDays, d))}>
          {parseDay(d).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric' })}
        </button>
      {/each}
    </div>
  </section>

  {#if planner.people.length > 1}
    <section>
      <h2>Whose plans</h2>
      <div class="chips">
        {#each planner.people as p (p.id)}
          <button aria-pressed={chosenPeople.has(p.id)} onclick={() => (chosenPeople = toggle(chosenPeople, p.id))}>
            {p.name}
          </button>
        {/each}
      </div>
    </section>
  {/if}

  <section>
    <h2>To cook</h2>
    {#if planned.length === 0}
      <p class="empty">Nothing planned on these days.</p>
    {:else}
      <ul class="card">
        {#each planned as r (r.path)}
          <li>
            <span class="title">{r.title}</span>
            <span class="small muted">
              {#if servingsOf(r.path)}
                {servingsText(r.portions)}{planner.people.length > 1
                  ? ` (${r.by.map((b) => `${name(b.person)} ${fmt(b.portions)}`).join(', ')})`
                  : ''}
              {:else}
                as written (the recipe doesn’t say how many it serves)
              {/if}
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <button class="btn primary create" onclick={create} disabled={creating || planned.length === 0}>
    {creating ? 'Making the list…' : 'Make shopping list'}
  </button>
</main>

<style>
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
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  .chips button {
    min-height: 40px;
    padding: 0 0.9rem;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--surface);
    font-weight: 600;
  }
  .chips button[aria-pressed='true'] {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: grid;
    padding: 0.6rem 1rem;
  }
  li + li {
    border-top: 1px solid var(--line);
  }
  .title {
    font-weight: 600;
  }
  .error {
    color: var(--danger);
  }
  .create {
    width: 100%;
    min-height: 52px;
  }
</style>
