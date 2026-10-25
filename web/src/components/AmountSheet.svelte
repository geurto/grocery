<script lang="ts">
  import { untrack } from 'svelte';
  import Sheet from './Sheet.svelte';
  import { amountText, parseNumber } from '../lib/foods';
  import type { Food } from '../lib/types';

  let {
    title,
    name,
    food,
    amount: initialAmount,
    unit: initialUnit,
    action = 'Add',
    onsave,
    onclose,
  }: {
    title: string;
    /** The food as typed. */
    name: string;
    /** For its piece weight; missing for a food this phone doesn't know yet. */
    food?: Food;
    amount?: number;
    unit?: string | null;
    action?: string;
    onsave: (amount: number, unit: string) => void | Promise<void>;
    onclose: () => void;
  } = $props();

  // A piece when the food has a piece weight, else grams.
  let amount = $state(
    untrack(() => (initialAmount !== undefined ? amountText(initialAmount) : food?.piece_g ? '1' : '100')),
  );
  let unit = $state(untrack(() => initialUnit ?? (initialAmount === undefined && !food?.piece_g ? 'g' : '')));
  let busy = $state(false);

  const UNITS = ['g', 'kg', 'ml', 'l', 'el', 'tl', 'stuks'];
  const value = $derived(parseNumber(amount));
  const pieces = $derived(['', 'stuk', 'stuks'].includes(unit.trim().toLowerCase()));
  const hint = $derived.by(() => {
    if (!food || food.negligible || value === undefined || !pieces) return null;
    return food.piece_g
      ? `≈ ${Math.round(value * food.piece_g)} g`
      : 'No piece weight known for this food: give grams to count its nutrition.';
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (value === undefined || value <= 0 || busy) return;
    busy = true;
    try {
      await onsave(value, unit.trim());
    } finally {
      busy = false;
    }
  }
</script>

<Sheet {title} {onclose}>
  <form onsubmit={submit} id="amount-form">
    <p class="name">{name}{#if food && food.name !== name}<span class="muted"> ({food.name})</span>{/if}</p>
    <div class="amount">
      <label>
        <span>Amount</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input class="field" bind:value={amount} inputmode="decimal" placeholder="1, 25 or 1/2" autofocus />
      </label>
      <label>
        <span>Unit <small class="muted">(empty: pieces)</small></span>
        <input class="field" bind:value={unit} list="amount-units" autocapitalize="none" placeholder="g, ml, el…" />
        <datalist id="amount-units">
          {#each UNITS as u (u)}<option value={u}></option>{/each}
        </datalist>
      </label>
    </div>
    {#if hint}<p class="small muted">{hint}</p>{/if}
  </form>

  {#snippet footer()}
    <button class="btn" type="button" onclick={onclose}>Cancel</button>
    <button class="btn primary" type="submit" form="amount-form" disabled={!value || busy}>{action}</button>
  {/snippet}
</Sheet>

<style>
  .name {
    font-weight: 600;
    margin: 0 0 0.9rem;
  }
  .amount {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.6rem;
  }
  .amount label {
    display: grid;
    gap: 0.3rem;
  }
  .amount span {
    font-size: 0.85rem;
    font-weight: 600;
  }
</style>
