<script lang="ts">
  import { untrack } from 'svelte';
  import Sheet from './Sheet.svelte';
  import { cookwareSnippet, ingredientSnippet, timerSnippet } from '../lib/editor';
  import type { Food } from '../lib/types';

  let {
    kind,
    selected = '',
    foods,
    oninsert,
    onclose,
  }: {
    kind: 'ingredient' | 'timer' | 'cookware';
    /** Text selected in the method: used as the name. */
    selected?: string;
    foods: Food[];
    oninsert: (snippet: string) => void;
    onclose: () => void;
  } = $props();

  let name = $state(untrack(() => selected.trim()));
  let amount = $state('');
  let unit = $state('');
  let note = $state('');
  let optional = $state(false);
  let timerUnit = $state<'seconds' | 'minutes' | 'hours'>('minutes');

  const names = $derived([...new Set(foods.flatMap((f) => [f.name, ...f.synonyms]))].sort());
  const UNITS = ['g', 'kg', 'ml', 'dl', 'l', 'el', 'tl', 'stuks', 'teentjes', 'blik', 'bos', 'snufje'];

  const snippet = $derived(
    kind === 'ingredient'
      ? name.trim()
        ? ingredientSnippet({ name, amount, unit, note, optional })
        : ''
      : kind === 'timer'
        ? timerSnippet({ amount, unit: timerUnit, name })
        : cookwareSnippet(name),
  );

  const title = $derived(
    kind === 'ingredient' ? 'Add an ingredient' : kind === 'timer' ? 'Add a timer' : 'Add cookware',
  );

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (snippet) oninsert(snippet);
  }
</script>

<Sheet {title} {onclose}>
  <form onsubmit={submit} id="insert-form">
    {#if kind === 'timer'}
      <div class="amount">
        <label>
          <span>For how long</span>
          <input class="field" inputmode="decimal" bind:value={amount} placeholder="10" />
        </label>
        <label>
          <span>&nbsp;</span>
          <select class="field" bind:value={timerUnit}>
            <option value="seconds">seconds</option>
            <option value="minutes">minutes</option>
            <option value="hours">hours</option>
          </select>
        </label>
      </div>
      <label class="row">
        <span>What for <small class="muted">(optional)</small></span>
        <input class="field" bind:value={name} placeholder="e.g. rijst" />
      </label>
    {:else}
      <label class="row">
        <span>{kind === 'cookware' ? 'Cookware' : 'Ingredient'}</span>
        <input
          class="field"
          bind:value={name}
          list={kind === 'ingredient' ? 'foods' : undefined}
          autocapitalize="none"
          placeholder={kind === 'cookware' ? 'e.g. grote pan' : 'e.g. ui'}
        />
        <datalist id="foods">
          {#each names as n (n)}<option value={n}></option>{/each}
        </datalist>
      </label>
    {/if}

    {#if kind === 'ingredient'}
      <div class="amount">
        <label>
          <span>Amount</span>
          <input class="field" bind:value={amount} placeholder="2, 1/2 or a word" />
        </label>
        <label>
          <span>Unit</span>
          <input class="field" bind:value={unit} list="units" autocapitalize="none" placeholder="g, el, tl…" />
          <datalist id="units">
            {#each UNITS as u (u)}<option value={u}></option>{/each}
          </datalist>
        </label>
      </div>
      <label class="row">
        <span>Note <small class="muted">(optional)</small></span>
        <input class="field" bind:value={note} placeholder="e.g. fijngesnipperd" />
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={optional} />
        <span>Optional</span>
      </label>
    {/if}

    {#if snippet}<p class="preview small muted">Adds <code>{snippet}</code></p>{/if}
  </form>

  {#snippet footer()}
    <button class="btn" type="button" onclick={onclose}>Cancel</button>
    <button class="btn primary" type="submit" form="insert-form" disabled={!snippet}>Insert</button>
  {/snippet}
</Sheet>

<style>
  .row,
  .amount label {
    display: grid;
    gap: 0.3rem;
    margin-bottom: 0.9rem;
  }
  .row > span,
  .amount span {
    font-size: 0.85rem;
    font-weight: 600;
  }
  .amount {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.6rem;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    min-height: 44px;
  }
  .check input {
    width: 22px;
    height: 22px;
    accent-color: var(--accent);
  }
  code {
    font-size: 0.95em;
    color: var(--text);
  }
  select.field {
    appearance: auto;
  }
</style>
