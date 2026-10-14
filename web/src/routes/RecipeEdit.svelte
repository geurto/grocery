<script lang="ts">
  import { tick, untrack } from 'svelte';
  import FoodSheet from '../components/FoodSheet.svelte';
  import Header from '../components/Header.svelte';
  import InsertSheet from '../components/InsertSheet.svelte';
  import { api, HttpError, OfflineError } from '../lib/api';
  import { insertAt, parseAmount, parseTags } from '../lib/editor';
  import { router } from '../lib/router.svelte';
  import { load, remove, save } from '../lib/storage';
  import { store } from '../lib/store.svelte';
  import type { CheckedIngredient, Food, RecipeCheck, RecipeSource } from '../lib/types';

  /** `path` edits that recipe; without it this makes a new one in `folder`. */
  let { path, folder: startFolder = '' }: { path?: string; folder?: string } = $props();

  const isNew = untrack(() => path === undefined);
  const draftKey = untrack(() => `draft:recipe:${path ?? 'new'}`);

  interface Draft {
    title: string;
    folder: string;
    servings: string;
    time: string;
    tags: string;
    extra: string;
    body: string;
    /** Version of the file the draft was made from. */
    version?: string;
  }

  let title = $state('');
  let folder = $state(untrack(() => startFolder));
  let servings = $state('');
  let time = $state('');
  let tags = $state('');
  let extra = $state('');
  let body = $state('');
  let version = $state<string | undefined>();
  let note = $state<string | null>(null);
  let restored = $state(false);

  let loading = $state(!isNew);
  let loadError = $state<string | null>(null);
  let saving = $state(false);
  let error = $state<string | null>(null);

  let folders = $state<string[]>([]);
  let foods = $state<Food[]>([]);
  let check = $state<RecipeCheck | null>(null);
  let checking = $state(false);
  let sheet = $state<null | 'ingredient' | 'timer' | 'cookware'>(null);
  let addFood = $state<string | null>(null);
  let selection = $state({ start: 0, end: 0, text: '' });
  let textarea = $state<HTMLTextAreaElement>();
  let ready = $state(false);

  function source(): RecipeSource {
    const s = parseAmount(servings);
    return {
      title: title.trim(),
      servings: typeof s === 'number' && s > 0 ? s : null,
      time: time.trim() || null,
      tags: parseTags(tags),
      extra,
      body,
    };
  }

  function apply(s: RecipeSource) {
    title = s.title;
    servings = s.servings === null ? '' : String(s.servings);
    time = s.time ?? '';
    tags = s.tags.join(', ');
    extra = s.extra;
    body = s.body;
  }

  // ---------------------------------------------------------------- loading

  $effect(() => {
    untrack(async () => {
      api.recipeFolders().then(
        (f) => (folders = f),
        () => {},
      );
      api.foods().then(
        (f) => (foods = f),
        () => {},
      );
      const draft = load<Draft | null>(draftKey, null);
      if (path === undefined) {
        if (draft) {
          title = draft.title;
          folder = draft.folder || startFolder;
          servings = draft.servings;
          time = draft.time;
          tags = draft.tags;
          extra = draft.extra;
          body = draft.body;
          restored = true;
        }
        ready = true;
        return;
      }
      try {
        const file = await api.recipeFile(path);
        version = file.version;
        note = file.note;
        if (draft && draft.version === file.version) {
          title = draft.title;
          servings = draft.servings;
          time = draft.time;
          tags = draft.tags;
          extra = draft.extra;
          body = draft.body;
          restored = true;
        } else {
          apply(file.source);
          if (draft) remove(draftKey); // the file changed since: the draft is stale
        }
      } catch (e) {
        loadError =
          e instanceof OfflineError
            ? 'Editing needs a connection to the server.'
            : e instanceof Error
              ? e.message
              : String(e);
      } finally {
        loading = false;
        ready = true;
      }
    });
  });

  // Keep what you typed if the page is closed or reloaded.
  $effect(() => {
    const draft: Draft = { title, folder, servings, time, tags, extra, body, version };
    if (!ready) return;
    untrack(() => {
      if (title || body) save(draftKey, draft);
    });
  });

  function discardDraft() {
    remove(draftKey);
    restored = false;
    if (path !== undefined) {
      api.recipeFile(path).then((file) => {
        version = file.version;
        apply(file.source);
      });
    } else {
      apply({ title: '', servings: null, time: null, tags: [], extra: '', body: '' });
    }
  }

  // ---------------------------------------------------------------- live check

  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    // Re-run on every field that's part of the recipe.
    void [title, servings, time, tags, extra, body];
    if (!ready) return;
    clearTimeout(timer);
    timer = setTimeout(() => untrack(() => void runCheck()), 500);
    return () => clearTimeout(timer);
  });

  async function runCheck() {
    if (!body.trim()) {
      check = null;
      return;
    }
    const mine = ++seq;
    checking = true;
    try {
      const result = await api.checkRecipe(source());
      if (mine === seq) check = result;
    } catch {
      // offline or server down: keep the last result
    } finally {
      if (mine === seq) checking = false;
    }
  }

  // ---------------------------------------------------------------- inserting

  function open(kind: 'ingredient' | 'timer' | 'cookware') {
    if (textarea) {
      const { selectionStart: start, selectionEnd: end } = textarea;
      selection = { start, end, text: body.slice(start, end) };
    } else {
      selection = { start: body.length, end: body.length, text: '' };
    }
    sheet = kind;
  }

  async function insert(snippet: string) {
    const r = insertAt(body, selection.start, selection.end, snippet);
    body = r.text;
    sheet = null;
    await tick();
    textarea?.focus();
    textarea?.setSelectionRange(r.cursor, r.cursor);
  }

  async function foodAdded() {
    addFood = null;
    api.foods().then((f) => (foods = f));
    await runCheck();
  }

  // ---------------------------------------------------------------- saving

  const unknown = $derived(check?.ingredients.filter((i) => i.status === 'unknown') ?? []);
  const canSave = $derived(!!title.trim() && !!body.trim() && !check?.error && !saving);

  async function submit() {
    error = null;
    if (!title.trim()) return void (error = 'A recipe needs a title.');
    if (servings.trim() && typeof parseAmount(servings) !== 'number') {
      return void (error = `Servings: “${servings}” isn't a number.`);
    }
    saving = true;
    try {
      const saved = isNew
        ? await api.createRecipe(folder.trim(), source())
        : await api.saveRecipe(path!, source(), version!);
      remove(draftKey);
      void store.refreshRecipes();
      router.go({ name: 'recipe', path: saved.path }, true);
    } catch (e) {
      error =
        e instanceof OfflineError
          ? 'Saving needs a connection to the server.'
          : e instanceof HttpError || e instanceof Error
            ? e.message
            : String(e);
    } finally {
      saving = false;
    }
  }

  function badge(i: CheckedIngredient): string | null {
    if (i.status === 'unknown') return null;
    if (i.nutrition === 'none') return 'no nutrition data';
    return null;
  }
</script>

<Header title={isNew ? 'New recipe' : 'Edit recipe'} back />

<main>
  {#if loading}
    <p class="empty">Loading…</p>
  {:else if loadError}
    <p class="empty">{loadError}</p>
  {:else}
    {#if restored}
      <p class="card notice-box small">
        Restored what you were typing.
        <button class="link" onclick={discardDraft}>Discard</button>
      </p>
    {/if}
    {#if note}
      <p class="card notice-box small">
        Couldn’t split this file into fields ({note}); the whole text is in the method box.
      </p>
    {/if}

    <label class="row">
      <span>Title</span>
      <input class="field" bind:value={title} placeholder="e.g. Tomatensoep" autocomplete="off" />
    </label>

    {#if isNew}
      <label class="row">
        <span>Folder <small class="muted">(under Recipes/; empty for the top)</small></span>
        <input class="field" bind:value={folder} list="folders" placeholder="e.g. Diner" autocomplete="off" />
        <datalist id="folders">
          {#each folders as f (f)}<option value={f}></option>{/each}
        </datalist>
      </label>
    {/if}

    <div class="two">
      <label class="row">
        <span>Servings</span>
        <input class="field" inputmode="decimal" bind:value={servings} placeholder="4" />
      </label>
      <label class="row">
        <span>Time <small class="muted">(optional)</small></span>
        <input class="field" bind:value={time} placeholder="40 min" />
      </label>
    </div>

    <label class="row">
      <span>Tags <small class="muted">(comma separated)</small></span>
      <input class="field" bind:value={tags} placeholder="diner, vegetarisch" autocapitalize="none" />
    </label>

    <div class="row">
      <span>Method</span>
      <div class="tools" role="toolbar" aria-label="Insert">
        <button type="button" class="btn" onclick={() => open('ingredient')}>@ Ingredient</button>
        <button type="button" class="btn" onclick={() => open('timer')}>~ Timer</button>
        <button type="button" class="btn" onclick={() => open('cookware')}># Cookware</button>
      </div>
      <textarea
        bind:this={textarea}
        bind:value={body}
        class="field method"
        rows="12"
        spellcheck="true"
        placeholder={'Snijd @ui{2} en bak met @olijfolie{1%el} in een #pan{}.\n\nLaat ~{10%minutes} sudderen.'}
        aria-describedby="method-help"
      ></textarea>
      <p class="small muted" id="method-help">
        One paragraph per step. <code>@name{'{'}amount%unit{'}'}</code> for ingredients,
        <code>~{'{'}10%minutes{'}'}</code> for timers, <code>#pan{'{}'}</code> for cookware.
      </p>
    </div>

    {#if check?.error}
      <div class="card problem" role="alert">
        <strong>This can’t be saved yet.</strong>
        <pre>{check.error}</pre>
      </div>
    {/if}

    {#if check && !check.error}
      <section aria-live="polite">
        <h2>
          Ingredients ({check.ingredients.length}) · {check.steps} {check.steps === 1 ? 'step' : 'steps'}
          {#if checking}<span class="muted small">checking…</span>{/if}
        </h2>
        {#if unknown.length}
          <p class="small muted">
            {unknown.length} not in your food list yet. Add them so they get an aisle, weights and
            nutrition.
          </p>
        {/if}
        <ul class="card ingredients">
          {#each check.ingredients as i (i.name)}
            <li>
              <span class="name">
                {i.name}{#if i.amount}<span class="muted"> — {i.amount}</span>{/if}
              </span>
              {#if i.status === 'unknown'}
                <button class="btn small-btn" onclick={() => (addFood = i.name)}>Add food</button>
              {:else}
                {#if badge(i)}<span class="tag warn">{badge(i)}</span>{/if}
                {#if i.weight === 'missing'}<span class="tag warn">no weight for this amount</span>{/if}
                {#if i.nutrition === 'negligible'}<span class="tag">negligible</span>{/if}
              {/if}
            </li>
          {/each}
        </ul>
        {#if check.warnings.length}<p class="small muted">⚠ {check.warnings.join(' · ')}</p>{/if}
      </section>
    {/if}

    <details class="extra">
      <summary>Other metadata <span class="muted small">(author, source, …)</span></summary>
      <textarea class="field" bind:value={extra} rows="4" spellcheck="false" aria-label="Other metadata (YAML)"
      ></textarea>
    </details>

    {#if error}<p class="error" role="alert">{error}</p>{/if}
  {/if}
</main>

{#if !loading && !loadError}
  <div class="savebar">
    <button class="btn" onclick={() => router.back()}>Cancel</button>
    <button class="btn primary" onclick={submit} disabled={!canSave}>
      {saving ? 'Saving…' : isNew ? 'Save recipe' : 'Save changes'}
    </button>
  </div>
{/if}

{#if sheet}
  <InsertSheet kind={sheet} selected={selection.text} {foods} oninsert={insert} onclose={() => (sheet = null)} />
{/if}
{#if addFood !== null}
  <FoodSheet name={addFood} onsaved={foodAdded} onclose={() => (addFood = null)} />
{/if}

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
  .two {
    display: grid;
    grid-template-columns: 1fr 1.4fr;
    gap: 0.6rem;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  .tools .btn {
    min-height: 40px;
    padding: 0 0.8rem;
    font-size: 0.9rem;
  }
  .method {
    min-height: 14rem;
    padding: 0.7rem 0.9rem;
    line-height: 1.5;
    resize: vertical;
    font-family: inherit;
  }
  code {
    font-size: 0.9em;
  }
  section {
    margin: 1rem 0;
  }
  h2 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 0 0 0.4rem 0.25rem;
  }
  .ingredients {
    list-style: none;
    margin: 0.4rem 0;
    padding: 0.2rem 0.9rem;
  }
  .ingredients li {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.4rem;
    padding: 0.5rem 0;
    min-height: 46px;
  }
  .ingredients li + li {
    border-top: 1px solid var(--line);
  }
  .name {
    flex: 1;
    min-width: 8rem;
  }
  .small-btn {
    min-height: 36px;
    padding: 0 0.8rem;
    font-size: 0.85rem;
  }
  .tag {
    font-size: 0.75rem;
    padding: 0.1em 0.6em;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
  }
  .tag.warn {
    background: var(--warn-soft);
    color: var(--warn);
  }
  .problem {
    padding: 0.8rem 1rem;
    margin: 0.5rem 0;
    border-color: var(--danger);
  }
  .problem pre {
    margin: 0.4rem 0 0;
    white-space: pre-wrap;
    font-size: 0.78rem;
    overflow-x: auto;
  }
  .notice-box {
    padding: 0.6rem 0.9rem;
    margin: 0 0 0.9rem;
  }
  .link {
    border: 0;
    background: none;
    color: var(--accent);
    font-weight: 600;
    padding: 0 0.3rem;
  }
  .extra summary {
    cursor: pointer;
    padding: 0.6rem 0;
    font-weight: 600;
  }
  .extra textarea {
    padding: 0.6rem 0.8rem;
    font-family: ui-monospace, monospace;
    font-size: 0.85rem;
  }
  .error {
    color: var(--danger);
  }
  .savebar {
    position: fixed;
    z-index: 15;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    gap: 0.5rem;
    max-width: 40rem;
    margin: 0 auto;
    padding: 0.6rem 1rem calc(0.6rem + env(safe-area-inset-bottom));
    background: var(--bg);
    border-top: 1px solid var(--line);
  }
  .savebar .btn {
    flex: 1;
    min-height: 50px;
  }
</style>
