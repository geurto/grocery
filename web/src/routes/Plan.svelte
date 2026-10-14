<script lang="ts">
  import { untrack } from 'svelte';
  import DayNutrition from '../components/DayNutrition.svelte';
  import Header from '../components/Header.svelte';
  import Stepper from '../components/Stepper.svelte';
  import { addDays, longDay, parseDay, today, weekDays, weekLabel, weekStart } from '../lib/dates';
  import { MEALS, planner } from '../lib/planner.svelte';
  import { href, router } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';
  import type { Meal, PlanEntry } from '../lib/types';

  let { day: wanted }: { day?: string } = $props();

  const day = $derived(wanted ?? today());
  const start = $derived(weekStart(day));
  const person = $derived(planner.person);
  const entries = $derived(person ? planner.entries(person.id, start) : undefined);
  const nutrition = $derived(person ? planner.weekNutrition(person.id, start) : undefined);
  const note = $derived(person ? planner.nutritionNote[`${person.id}|${start}`] : undefined);
  const todays = $derived((entries ?? []).filter((e) => e.day === day));
  const dayNutrition = $derived(nutrition?.days.find((d) => d.day === day));

  let newName = $state('');
  let adding = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    untrack(() => void planner.refreshPeople());
  });

  $effect(() => {
    const [p, s] = [person?.id, start];
    if (p !== undefined) untrack(() => void planner.refreshWeek(p, s));
  });

  function go(d: string) {
    router.go({ name: 'plan', day: d === today() ? undefined : d }, true);
  }

  async function addPerson(e: SubmitEvent) {
    e.preventDefault();
    if (!newName.trim()) return;
    error = null;
    try {
      await planner.addPerson(newName);
      newName = '';
      adding = false;
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    }
  }

  async function act(fn: () => Promise<void>) {
    try {
      await fn();
    } catch (err) {
      store.notice = err instanceof Error ? err.message : String(err);
    }
  }

  const kcal = (d: string) => nutrition?.days.find((x) => x.day === d)?.nutrients.ENERCC;
  const entryNutrition = (e: PlanEntry) => dayNutrition?.entries.find((x) => x.id === e.id);
  const inMeal = (meal: Meal | null) => todays.filter((e) => e.meal === meal);
</script>

<Header title="Plan" />

<main>
  {#if planner.people.length === 0 || adding}
    <form class="card who" onsubmit={addPerson}>
      <p>
        {planner.people.length === 0
          ? 'Plans are per person, so each of you sees your own calories. Who’s first?'
          : 'Add someone with their own plan.'}
      </p>
      <input class="field" bind:value={newName} placeholder="Name" aria-label="Name" maxlength="40" />
      {#if error}<p class="error small" role="alert">{error}</p>{/if}
      <div class="row">
        <button class="btn primary" type="submit" disabled={!newName.trim()}>Add</button>
        {#if planner.people.length}<button class="btn" type="button" onclick={() => (adding = false)}>Cancel</button>{/if}
      </div>
    </form>
  {/if}

  {#if person}
    <div class="people" role="tablist" aria-label="Whose plan">
      {#each planner.people as p (p.id)}
        <button role="tab" aria-selected={p.id === person.id} onclick={() => planner.select(p.id)}>{p.name}</button>
      {/each}
      {#if !adding}<button class="add" onclick={() => (adding = true)} aria-label="Add a person">+</button>{/if}
    </div>

    <div class="week">
      <button class="nav" onclick={() => go(addDays(day, -7))} aria-label="Previous week">‹</button>
      <span class="label">{weekLabel(start)}</span>
      <button class="nav" onclick={() => go(addDays(day, 7))} aria-label="Next week">›</button>
    </div>
    <div class="days">
      {#each weekDays(start) as d (d)}
        {@const k = kcal(d)}
        <button class:on={d === day} class:today={d === today()} onclick={() => go(d)} aria-pressed={d === day}>
          <span class="dow">{parseDay(d).toLocaleDateString(undefined, { weekday: 'short' })}</span>
          <span class="date">{parseDay(d).getDate()}</span>
          <span class="kcal">{k ? `${Math.round(k.value)}${k.lower_bound ? '*' : ''}` : ''}</span>
        </button>
      {/each}
    </div>

    <h2 class="day">{longDay(day)}</h2>

    {#each [...MEALS, { meal: null, label: 'Other' }] as m (m.meal)}
      {@const items = inMeal(m.meal)}
      {#if m.meal !== null || items.length}
        <section class="meal">
          <div class="meal-head">
            <h3>{m.label}</h3>
            {#if m.meal}
              <a class="add-entry" href={href({ name: 'plan-add', day, meal: m.meal })} aria-label="Add to {m.label}">+ Add</a>
            {/if}
          </div>
          {#if items.length}
            <ul class="card">
              {#each items as e (e.id)}
                {@const n = entryNutrition(e)}
                <li>
                  <div class="entry">
                    <a class="title" href={href({ name: 'recipe', path: e.path })}>{e.title}</a>
                    <span class="small muted">
                      {#if n?.problem}<span class="warn">Not counted: {n.problem}</span>
                      {:else if n?.kcal != null}{Math.round(n.kcal)} kcal{n.kcal_lower_bound ? '*' : ''}
                        {#if n.coverage !== null && n.coverage < 0.9}
                          · <span class="warn">{Math.round(n.coverage * 100)}% of ingredients counted</span>
                        {/if}
                      {/if}
                    </span>
                  </div>
                  <Stepper
                    value={e.portions}
                    min={0.5}
                    max={20}
                    step={0.5}
                    label="portions"
                    onchange={(v) => act(() => planner.setPortions(e, v))}
                  />
                  <button class="remove" onclick={() => act(() => planner.remove(e))} aria-label="Remove {e.title}">×</button>
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {/if}
    {/each}

    {#if note}
      <p class="small muted">{note}</p>
    {:else if dayNutrition && nutrition}
      <h2 class="section-title">Nutrition for {person.name}</h2>
      <DayNutrition day={dayNutrition} rows={nutrition.rows} citation={nutrition.citation} />
    {/if}

    <div class="actions">
      <a class="btn" href={href({ name: 'plan-list', start })}>Shopping list for this week…</a>
    </div>
  {/if}
</main>

<style>
  .who {
    padding: 1rem;
    margin-bottom: 1rem;
  }
  .who p {
    margin: 0 0 0.75rem;
  }
  .row {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.75rem;
  }
  .error,
  .warn {
    color: var(--warn);
  }
  .people {
    display: flex;
    gap: 0.4rem;
    overflow-x: auto;
    margin-bottom: 0.75rem;
  }
  .people button {
    min-height: 40px;
    padding: 0 1rem;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--surface);
    font-weight: 600;
    white-space: nowrap;
  }
  .people button[aria-selected='true'] {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  .people .add {
    width: 40px;
    padding: 0;
    font-size: 1.2rem;
  }
  .week {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .week .label {
    font-weight: 650;
  }
  .nav {
    width: 44px;
    height: 44px;
    border: 0;
    background: none;
    font-size: 1.6rem;
    line-height: 1;
  }
  .days {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 0.25rem;
    margin: 0.25rem 0 1rem;
  }
  .days button {
    display: grid;
    justify-items: center;
    gap: 0.05rem;
    padding: 0.4rem 0;
    border-radius: 12px;
    border: 1px solid transparent;
    background: none;
    min-width: 0;
  }
  .days .dow {
    font-size: 0.7rem;
    text-transform: uppercase;
    color: var(--muted);
  }
  .days .date {
    font-weight: 700;
    font-size: 1.05rem;
  }
  .days .kcal {
    font-size: 0.65rem;
    color: var(--muted);
    min-height: 1em;
    font-variant-numeric: tabular-nums;
  }
  .days .today .date {
    color: var(--accent);
  }
  .days .on {
    background: var(--surface);
    border-color: var(--accent);
  }
  .day {
    font-size: 1.15rem;
    margin: 0 0.25rem 0.5rem;
  }
  .meal {
    margin-bottom: 0.75rem;
  }
  .meal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h3,
  .section-title {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 0 0 0 0.25rem;
  }
  .section-title {
    margin: 1.5rem 0 0.5rem 0.25rem;
  }
  .add-entry {
    padding: 0.6rem 0.25rem;
    font-weight: 600;
    text-decoration: none;
  }
  .meal ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .meal li {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.5rem 0.25rem 0.5rem 0.9rem;
  }
  .meal li + li {
    border-top: 1px solid var(--line);
  }
  .entry {
    flex: 1;
    min-width: 0;
    display: grid;
  }
  .entry .title {
    font-weight: 600;
    color: inherit;
    text-decoration: none;
  }
  .remove {
    width: 40px;
    height: 40px;
    border: 0;
    background: none;
    font-size: 1.3rem;
    color: var(--muted);
  }
  .actions {
    margin-top: 1.25rem;
  }
</style>
