<script lang="ts">
  import { untrack } from 'svelte';
  import Sheet from './Sheet.svelte';
  import { api, HttpError, OfflineError } from '../lib/api';
  import type { FoodNutrition } from '../lib/types';

  let {
    name,
    onclose,
    onadd,
  }: {
    name: string;
    onclose: () => void;
    /** Show an "Add to list" button. */
    onadd?: () => void;
  } = $props();

  let info = $state<FoodNutrition | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    const n = name;
    untrack(async () => {
      info = null;
      error = null;
      try {
        info = await api.foodNutrition(n);
      } catch (e) {
        error =
          e instanceof OfflineError
            ? 'Nutrition needs a connection to the server.'
            : e instanceof HttpError && e.status === 409
              ? 'No NEVO data on the server yet (run grocery import-nevo).'
              : e instanceof HttpError && e.status === 404
                ? 'This isn’t in your food list.'
                : e instanceof Error
                  ? e.message
                  : String(e);
      }
    });
  });

  const firstSection = $derived(info?.nutrients.findIndex((r) => r.section) ?? -1);
  const macros = $derived(info ? (firstSection < 0 ? info.nutrients : info.nutrients.slice(0, firstSection)) : []);
  const micros = $derived(info && firstSection >= 0 ? info.nutrients.slice(firstSection) : []);
  const per = $derived(info?.per === '100ml' ? '100 ml' : '100 g');

  function fmt(v: number, unit: string): string {
    if (unit === 'kcal' || v >= 100) return Math.round(v).toLocaleString();
    return v >= 1 ? v.toFixed(1) : v.toFixed(2);
  }
</script>

{#snippet rows(list: FoodNutrition['nutrients'])}
  <ul class="rows">
    {#each list as r (r.code)}
      <li class:indent={r.indent}>
        {#if r.section}<h3>{r.section}</h3>{/if}
        <span>{r.label}</span>
        <span class="value">{r.value === null ? '–' : `${fmt(r.value, r.unit)} ${r.unit}`}</span>
      </li>
    {/each}
  </ul>
{/snippet}

<Sheet title={info?.food ?? name} {onclose}>
  {#if error}
    <p class="muted">{error}</p>
  {:else if !info}
    <p class="muted">Loading…</p>
  {:else if info.source === 'negligible'}
    <p>Marked <strong>negligible</strong>: left out of nutrition, like a pinch of dried herbs.</p>
  {:else if info.source === 'none' || !info.nutrients.length}
    <p class="muted">No nutrition data for this food yet. Add a NEVO code or your own values in Food/aliases.yaml.</p>
  {:else}
    <p class="small muted source">
      Per {per}{info.piece_g ? ` · one piece is about ${info.piece_g} g` : ''}.
      {#if info.nevo}From NEVO: {info.nevo.name} ({info.nevo.code}).{/if}
      {#if info.source === 'custom'}Your own values{info.own_source ? ` (${info.own_source})` : ''}, not NEVO data.{/if}
    </p>
    {@render rows(macros)}
    {#if micros.length}
      <details>
        <summary>Vitamins &amp; minerals</summary>
        {@render rows(micros)}
      </details>
    {/if}
    <p class="small muted">– means unknown, not zero.</p>
    {#if info.citation}<p class="citation">{info.citation}</p>{/if}
  {/if}

  {#snippet footer()}
    {#if onadd}
      <button class="btn" onclick={onclose}>Close</button>
      <button class="btn primary" onclick={onadd}>Add to list</button>
    {:else}
      <button class="btn" onclick={onclose}>Close</button>
    {/if}
  {/snippet}
</Sheet>

<style>
  .source {
    margin: 0 0 0.6rem;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .rows li {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.4rem 0;
    border-top: 1px solid var(--line);
    font-variant-numeric: tabular-nums;
  }
  .rows h3 {
    width: 100%;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 0.5rem 0 0.2rem;
  }
  .indent span:first-child {
    padding-left: 1rem;
    color: var(--muted);
  }
  details summary {
    cursor: pointer;
    font-weight: 600;
    padding: 0.6rem 0;
  }
  .citation {
    font-size: 0.75rem;
    color: var(--muted);
    margin: 0.5rem 0 0;
  }
</style>
