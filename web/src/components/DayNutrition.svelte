<script lang="ts">
  import type { DayNutrition, NutrientRow } from '../lib/types';

  let { day, rows, citation }: { day: DayNutrition; rows: NutrientRow[]; citation: string } = $props();

  // Label-style rows first; vitamins and minerals fold away.
  const firstSection = $derived(rows.findIndex((r) => r.section));
  const macros = $derived(firstSection < 0 ? rows : rows.slice(0, firstSection));
  const micros = $derived(firstSection < 0 ? [] : rows.slice(firstSection));
  const energy = $derived(day.nutrients.ENERCC);
  const energyRow = $derived(rows.find((r) => r.code === 'ENERCC'));
  const counted = $derived(day.entries.filter((e) => !e.problem).length);

  function fmt(v: number, unit: string): string {
    if (unit === 'kcal' || unit === 'kJ' || v >= 100) return Math.round(v).toLocaleString();
    return v >= 1 ? v.toFixed(1) : v.toFixed(2);
  }
</script>

{#snippet row(r: NutrientRow)}
  {@const n = day.nutrients[r.code]}
  {@const pct = n && r.ri ? (100 * n.value) / r.ri : null}
  <li class:indent={r.indent}>
    {#if r.section}<h3>{r.section}</h3>{/if}
    <span class="label">{r.label}</span>
    <span class="value">
      {#if n}{fmt(n.value, r.unit)} {r.unit}{#if n.lower_bound}<abbr title="at least: some foods have no value">*</abbr>{/if}{:else}–{/if}
    </span>
    <span class="pct">{pct === null ? '' : `${Math.round(pct)}%`}</span>
    {#if pct !== null}
      <span class="bar" aria-hidden="true"><span style:width="{Math.min(100, pct)}%" class:over={pct > 100}></span></span>
    {/if}
  </li>
{/snippet}

<section class="card nutrition">
  {#if counted === 0}
    <p class="muted small none">Nothing counted for this day yet.</p>
  {:else}
    <div class="energy">
      <span class="big">
        {energy ? fmt(energy.value, 'kcal') : '–'}<small> kcal</small>{#if energy?.lower_bound}<abbr
            title="at least: some foods have no value">*</abbr
          >{/if}
      </span>
      {#if energy && energyRow?.ri}
        <span class="muted small">{Math.round((100 * energy.value) / energyRow.ri)}% of {energyRow.ri.toLocaleString()} kcal</span>
      {/if}
    </div>
    <ul class="rows">
      {#each macros.filter((r) => r.code !== 'ENERCC') as r (r.code)}{@render row(r)}{/each}
    </ul>
    {#if micros.length}
      <details>
        <summary>Vitamins &amp; minerals</summary>
        <ul class="rows">
          {#each micros as r (r.code)}{@render row(r)}{/each}
        </ul>
      </details>
    {/if}

    {#if day.missing.length}
      <details class="notes">
        <summary>* at least: {day.missing.length} {day.missing.length === 1 ? 'food has' : 'foods have'} no value for some nutrients</summary>
        <ul>
          {#each day.missing as m (m.food)}<li><strong>{m.food}</strong>: {m.nutrients.join(', ')}</li>{/each}
        </ul>
      </details>
    {/if}
    {#if day.own_foods.length}
      <p class="small muted">Own values (custom.yaml), not NEVO data: {day.own_foods.join(', ')}</p>
    {/if}
  {/if}
  <p class="citation">% of EU reference intakes for an adult. {citation}</p>
</section>

<style>
  .nutrition {
    padding: 1rem;
  }
  .none {
    margin: 0 0 0.5rem;
  }
  .energy {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    flex-wrap: wrap;
    margin-bottom: 0.5rem;
  }
  .big {
    font-size: 2rem;
    font-weight: 750;
    font-variant-numeric: tabular-nums;
  }
  .big small {
    font-size: 1rem;
    font-weight: 600;
    color: var(--muted);
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .rows li {
    display: grid;
    grid-template-columns: 1fr auto 3.2rem;
    align-items: center;
    column-gap: 0.5rem;
    padding: 0.35rem 0 0.45rem;
    border-top: 1px solid var(--line);
    font-variant-numeric: tabular-nums;
  }
  .rows h3 {
    grid-column: 1 / -1;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 0.6rem 0 0.3rem;
  }
  .indent .label {
    padding-left: 1rem;
    color: var(--muted);
  }
  .value {
    text-align: right;
  }
  .pct {
    text-align: right;
    color: var(--muted);
    font-size: 0.85rem;
  }
  .bar {
    grid-column: 1 / -1;
    height: 4px;
    margin-top: 0.3rem;
    border-radius: 2px;
    background: var(--surface-2);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .bar span.over {
    background: var(--warn);
  }
  abbr {
    text-decoration: none;
    color: var(--warn);
    margin-left: 0.1em;
  }
  details summary {
    cursor: pointer;
    padding: 0.6rem 0;
    font-weight: 600;
  }
  .notes summary {
    font-weight: 500;
    color: var(--warn);
    font-size: 0.9rem;
  }
  .notes ul {
    margin: 0 0 0.5rem;
    padding-left: 1.2rem;
    font-size: 0.9rem;
  }
  .citation {
    margin: 0.75rem 0 0;
    font-size: 0.75rem;
    color: var(--muted);
  }
</style>
