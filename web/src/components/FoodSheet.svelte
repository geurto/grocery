<script lang="ts">
  import { untrack } from 'svelte';
  import Sheet from './Sheet.svelte';
  import { api, HttpError, OfflineError } from '../lib/api';
  import { parseAmount, parseUnits } from '../lib/editor';
  import type { NevoHit, NewFood, OwnValues } from '../lib/types';

  let {
    name: initialName = '',
    onsaved,
    onclose,
  }: { name?: string; onsaved: (name: string) => void; onclose: () => void } = $props();

  type Source = 'nevo' | 'own' | 'negligible' | 'none';

  let name = $state(untrack(() => initialName));
  let aisle = $state('');
  let synonyms = $state('');
  let source = $state<Source>('nevo');
  let aisles = $state<string[]>([]);

  // NEVO
  let query = $state(untrack(() => initialName));
  let hits = $state<NevoHit[]>([]);
  let chosen = $state<NevoHit | null>(null);
  let searchNote = $state<string | null>(null);
  let searching = $state(false);

  // Own values
  let per = $state<'100g' | '100ml'>('100g');
  let ownSource = $state('');
  let own = $state({
    energy_kcal: '',
    fat: '',
    saturated_fat: '',
    carbohydrate: '',
    sugars: '',
    fibre: '',
    protein: '',
    salt: '',
  });

  // Weights
  let pieceG = $state('');
  let density = $state('');
  let units = $state('');

  let saving = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    untrack(() => {
      api.aisles().then(
        (a) => (aisles = a),
        () => {},
      );
      void search();
    });
  });

  let timer: ReturnType<typeof setTimeout> | undefined;
  function onquery() {
    clearTimeout(timer);
    timer = setTimeout(() => void search(), 250);
  }

  async function search() {
    const q = query.trim();
    if (!q) {
      hits = [];
      return;
    }
    searching = true;
    try {
      hits = await api.nevoSearch(q);
      searchNote = hits.length ? null : 'Nothing found. Try another word, or use your own values.';
    } catch (e) {
      hits = [];
      searchNote =
        e instanceof HttpError && e.status === 409
          ? 'No NEVO data on the server yet (run grocery import-nevo). Use your own values or skip nutrition.'
          : e instanceof OfflineError
            ? 'Searching needs a connection.'
            : String(e instanceof Error ? e.message : e);
    } finally {
      searching = false;
    }
  }

  function numberOf(label: string, text: string): number | undefined {
    const v = parseAmount(text);
    if (v === null) throw new Error(`${label}: “${text}” isn't a number`);
    return v;
  }

  function build(): NewFood {
    const food: NewFood = { name: name.trim() };
    if (!food.name) throw new Error('A food needs a name');
    if (aisle.trim()) food.aisle = aisle.trim();
    const syn = synonyms.split(',').map((s) => s.trim()).filter(Boolean);
    if (syn.length) food.synonyms = syn;

    if (source === 'nevo') {
      if (!chosen) throw new Error('Pick a NEVO food, or choose another source of nutrition');
      food.nevo = chosen.code;
    } else if (source === 'negligible') {
      food.negligible = true;
    } else if (source === 'own') {
      const values: OwnValues = { per, source: ownSource.trim() || undefined };
      const fields: [keyof typeof own, string][] = [
        ['energy_kcal', 'Energy'],
        ['fat', 'Fat'],
        ['saturated_fat', 'Saturated fat'],
        ['carbohydrate', 'Carbohydrate'],
        ['sugars', 'Sugars'],
        ['fibre', 'Fibre'],
        ['protein', 'Protein'],
        ['salt', 'Salt'],
      ];
      let any = false;
      for (const [key, label] of fields) {
        const v = numberOf(label, own[key]);
        if (v !== undefined) {
          values[key] = v;
          any = true;
        }
      }
      if (!any) throw new Error('Enter at least one value from the package');
      food.own = values;
    }

    const piece = numberOf('Piece weight', pieceG);
    if (piece !== undefined) food.piece_g = piece;
    const d = numberOf('Density', density);
    if (d !== undefined) food.density = d;
    if (units.trim()) {
      const parsed = parseUnits(units);
      if (!parsed) throw new Error('Unit weights: write them like “clove=5, blik=400”');
      food.units = parsed;
    }
    return food;
  }

  async function save() {
    error = null;
    let food: NewFood;
    try {
      food = build();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      return;
    }
    saving = true;
    try {
      const added = await api.addFood(food);
      onsaved(added.name);
    } catch (e) {
      error =
        e instanceof OfflineError
          ? 'Adding a food needs a connection to the server.'
          : e instanceof Error
            ? e.message
            : String(e);
    } finally {
      saving = false;
    }
  }
</script>

<Sheet title="Add a food" {onclose}>
  <label class="row">
    <span>Name, as written in recipes</span>
    <input class="field" bind:value={name} autocapitalize="none" />
  </label>
  <label class="row">
    <span>Other names <small class="muted">(comma separated)</small></span>
    <input class="field" bind:value={synonyms} autocapitalize="none" placeholder="e.g. gedroogde tijm, thyme" />
  </label>
  <label class="row">
    <span>Shop aisle</span>
    <input class="field" bind:value={aisle} list="aisles" placeholder="pick one or type a new one" />
    <datalist id="aisles">
      {#each aisles as a (a)}<option value={a}></option>{/each}
    </datalist>
  </label>

  <fieldset>
    <legend>Nutrition</legend>
    <div class="choices" role="radiogroup">
      {#each [['nevo', 'From NEVO'], ['own', 'My own values'], ['negligible', 'Negligible'], ['none', 'Not now']] as [value, label] (value)}
        <button
          type="button"
          role="radio"
          aria-checked={source === value}
          class:on={source === value}
          onclick={() => (source = value as Source)}>{label}</button
        >
      {/each}
    </div>

    {#if source === 'nevo'}
      <input
        class="field"
        type="search"
        bind:value={query}
        oninput={onquery}
        placeholder="Search NEVO (Dutch or English)"
        aria-label="Search NEVO"
      />
      {#if searchNote}<p class="small muted">{searchNote}</p>{/if}
      {#if hits.length}
        <ul class="hits card" aria-busy={searching}>
          {#each hits as h (h.code)}
            <li>
              <button type="button" class:on={chosen?.code === h.code} onclick={() => (chosen = h)}>
                <span class="n">{h.name_nl}</span>
                <span class="small muted">
                  {h.name_en}{h.kcal !== null ? ` · ${Math.round(h.kcal)} kcal` : ''} · {h.code}
                </span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if chosen}<p class="small">Using <strong>{chosen.name_nl}</strong> ({chosen.code}).</p>{/if}
    {:else if source === 'own'}
      <p class="small muted">As printed on the package, per:</p>
      <div class="choices">
        <button type="button" class:on={per === '100g'} onclick={() => (per = '100g')}>100 g</button>
        <button type="button" class:on={per === '100ml'} onclick={() => (per = '100ml')}>100 ml</button>
      </div>
      <div class="grid">
        {#each [['energy_kcal', 'Energy kcal'], ['fat', 'Fat g'], ['saturated_fat', 'of which saturated'], ['carbohydrate', 'Carbohydrate g'], ['sugars', 'of which sugars'], ['fibre', 'Fibre g'], ['protein', 'Protein g'], ['salt', 'Salt g']] as [key, label] (key)}
          <label>
            <span class="small">{label}</span>
            <input
              class="field"
              inputmode="decimal"
              bind:value={own[key as keyof typeof own]}
              placeholder="–"
            />
          </label>
        {/each}
      </div>
      <p class="small muted">Leave out what the package doesn't list: unknown isn't counted as 0.</p>
      <input class="field" bind:value={ownSource} placeholder="Where from, e.g. package, Albert Heijn" aria-label="Source" />
    {:else if source === 'negligible'}
      <p class="small muted">
        For pinches of dried herbs and spices: left out of nutrition without a warning.
      </p>
    {:else}
      <p class="small muted">It will show as “no nutrition data” until you add some.</p>
    {/if}
  </fieldset>

  <details>
    <summary>Weights (to count spoons and pieces)</summary>
    <div class="grid">
      <label>
        <span class="small">One piece, g</span>
        <input class="field" inputmode="decimal" bind:value={pieceG} placeholder="e.g. 110" />
      </label>
      <label>
        <span class="small">Density, g per ml</span>
        <input class="field" inputmode="decimal" bind:value={density} placeholder="e.g. 0.92" />
      </label>
    </div>
    <label class="row">
      <span class="small">Other units, g each</span>
      <input class="field" bind:value={units} placeholder="clove=5, blik=400" />
    </label>
    <p class="small muted">
      A density lets el, tl and ml become grams (oil 0.92, soy sauce 1.15, ground spices about 0.45).
    </p>
  </details>

  {#if error}<p class="error" role="alert">{error}</p>{/if}

  {#snippet footer()}
    <button class="btn" onclick={onclose}>Cancel</button>
    <button class="btn primary" onclick={save} disabled={saving}>{saving ? 'Saving…' : 'Add food'}</button>
  {/snippet}
</Sheet>

<style>
  .row {
    display: grid;
    gap: 0.3rem;
    margin-bottom: 0.9rem;
  }
  .row > span {
    font-size: 0.85rem;
    font-weight: 600;
  }
  fieldset {
    border: 0;
    padding: 0;
    margin: 0 0 0.9rem;
  }
  legend {
    font-size: 0.85rem;
    font-weight: 600;
    margin-bottom: 0.4rem;
    padding: 0;
  }
  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    margin-bottom: 0.7rem;
  }
  .choices button {
    min-height: 40px;
    padding: 0 0.9rem;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--surface);
    font-weight: 600;
  }
  .choices button.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
  }
  .hits {
    list-style: none;
    margin: 0.6rem 0;
    padding: 0;
    max-height: 14rem;
    overflow-y: auto;
  }
  .hits li + li {
    border-top: 1px solid var(--line);
  }
  .hits button {
    width: 100%;
    display: grid;
    gap: 0.05rem;
    text-align: left;
    padding: 0.5rem 0.8rem;
    border: 0;
    background: none;
    min-height: 48px;
  }
  .hits button.on {
    background: var(--accent-soft);
  }
  .n {
    font-weight: 600;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.6rem;
    margin: 0.6rem 0;
  }
  .grid label {
    display: grid;
    gap: 0.2rem;
  }
  details summary {
    cursor: pointer;
    font-weight: 600;
    padding: 0.5rem 0;
  }
  .error {
    color: var(--danger);
    margin: 0.5rem 0 0;
  }
</style>
