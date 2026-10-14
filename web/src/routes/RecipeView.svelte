<script lang="ts">
  import { untrack } from 'svelte';
  import Header from '../components/Header.svelte';
  import Stepper from '../components/Stepper.svelte';
  import { api, HttpError, OfflineError } from '../lib/api';
  import { copyText } from '../lib/device';
  import { draft } from '../lib/draft.svelte';
  import { router } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';
  import type { Recipe } from '../lib/types';

  let { path, servings: wanted }: { path: string; servings?: number } = $props();

  // Initial values only: App re-creates this component for another recipe,
  // and the effects below follow servings changes.
  let recipe = $state<Recipe | undefined>(untrack(() => store.cachedRecipe(path, wanted)));
  let missing = $state(false);
  let servings = $state(untrack(() => wanted ?? 0));
  let nutrition = $state<string | null>(null);
  let nutritionNote = $state<string | null>(null);
  let toast = $state<string | null>(null);

  function load(p: string, s: number | undefined) {
    recipe = store.cachedRecipe(p, s) ?? recipe;
    missing = false;
    store.recipe(p, s).then(
      (r) => {
        if (r) {
          recipe = r;
          if (!servings && r.servings) servings = r.servings;
        } else if (!recipe) missing = true;
      },
      (e) => {
        if (!recipe) missing = true;
        toast = e instanceof HttpError ? e.message : String(e);
      },
    );
  }

  // (Re)load whenever the path or servings in the URL change.
  $effect(() => {
    const p = path;
    const s = wanted;
    untrack(() => load(p, s));
  });

  // Stepper → URL (replace, so "back" leaves the recipe rather than undoing steps).
  $effect(() => {
    if (!recipe?.servings || !servings) return;
    const s = servings === recipe.servings ? undefined : servings;
    if (s !== wanted) router.go({ name: 'recipe', path, servings: s }, true);
  });

  const inDraft = $derived(draft.picks.some((p) => p.path === path));

  function addToDraft() {
    if (!recipe) return;
    if (!inDraft) {
      draft.picks.push({ path, title: recipe.title, servings: recipe.servings ? servings || recipe.servings : null });
    }
    router.go({ name: 'new' });
  }

  async function showNutrition() {
    nutritionNote = null;
    try {
      nutrition = await api.nutrition(path);
    } catch (e) {
      nutritionNote =
        e instanceof OfflineError
          ? 'Nutrition needs a connection.'
          : e instanceof HttpError && e.status === 409
            ? 'No NEVO data on the server yet (run grocery import-nevo).'
            : e instanceof Error
              ? e.message
              : String(e);
    }
  }

  async function copyMarkdown() {
    try {
      const md = await api.recipeText(path, 'md', wanted);
      toast = (await copyText(md)) ? 'Copied as Markdown' : 'Couldn’t copy';
    } catch (e) {
      toast = e instanceof OfflineError ? 'Needs a connection' : String(e);
    }
    setTimeout(() => (toast = null), 2500);
  }
</script>

<Header title={recipe?.title ?? 'Recipe'} back />

<main>
  {#if !recipe}
    <p class="empty">{missing ? 'This recipe isn’t on this phone yet, and the server can’t be reached.' : 'Loading…'}</p>
  {:else if recipe.error}
    <div class="card problem">
      <p><strong>This recipe doesn’t parse.</strong> Fix the .cook file and it updates here.</p>
      <pre>{recipe.error}</pre>
    </div>
  {:else if recipe.display}
    {@const d = recipe.display}
    {#if recipe.description}<p class="description">{recipe.description}</p>{/if}

    <div class="toolbar">
      {#if recipe.servings}
        <span class="serves">Serves <Stepper bind:value={servings} label="servings" /></span>
      {:else}
        <span class="small muted">No servings in the recipe, so it can’t be scaled.</span>
      {/if}
    </div>
    {#if recipe.tags.length}
      <p class="tags">
        {#each recipe.tags as tag (tag)}<span class="tag">{tag}</span>{/each}
      </p>
    {/if}

    <section>
      <h2>Ingredients</h2>
      <ul class="card ingredients">
        {#each d.ingredients as line, i (i)}
          <li>{line}</li>
        {/each}
      </ul>
    </section>

    {#if d.sections.some((s) => s.steps.length)}
      <section>
        <h2>Method</h2>
        {#each d.sections as section, si (si)}
          {#if section.name}<h3>{section.name}</h3>{/if}
          <ol>
            {#each section.steps as step, i (i)}
              {#if step.type === 'step'}
                <li>{step.text}</li>
              {:else}
                <p class="note">{step.text}</p>
              {/if}
            {/each}
          </ol>
        {/each}
      </section>
    {/if}

    {#if recipe.warnings?.length}
      <p class="small muted">⚠ {recipe.warnings.join(' · ')}</p>
    {/if}

    <div class="actions">
      <button class="btn primary" onclick={addToDraft}>{inDraft ? 'Go to new list' : 'Add to a new list'}</button>
      <button class="btn" onclick={copyMarkdown}>Copy as Markdown</button>
      {#if nutrition === null}
        <button class="btn" onclick={showNutrition}>Nutrition</button>
      {/if}
    </div>
    {#if nutritionNote}<p class="small muted">{nutritionNote}</p>{/if}
    {#if nutrition !== null}
      <section>
        <h2>Nutrition per serving</h2>
        <pre class="card nutrition">{nutrition}</pre>
      </section>
    {/if}
  {/if}
</main>

{#if toast}
  <div class="toast" role="status">{toast}</div>
{/if}

<style>
  .description {
    margin: 0.25rem 0 0.75rem;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin: 0.5rem 0;
  }
  .serves {
    display: inline-flex;
    align-items: center;
    gap: 0.6rem;
    font-weight: 600;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    margin: 0.5rem 0 1rem;
  }
  .tag {
    font-size: 0.8rem;
    padding: 0.15em 0.6em;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
  }
  section {
    margin: 1.25rem 0;
  }
  h2 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 0 0 0.5rem 0.25rem;
  }
  h3 {
    font-size: 1rem;
    margin: 1rem 0 0.5rem;
  }
  .ingredients {
    list-style: none;
    margin: 0;
    padding: 0.25rem 1rem;
  }
  .ingredients li {
    padding: 0.5rem 0;
    border-top: 1px solid var(--line);
  }
  .ingredients li:first-child {
    border-top: 0;
  }
  ol {
    margin: 0;
    padding-left: 1.5rem;
  }
  ol li {
    margin-bottom: 0.8rem;
    padding-left: 0.25rem;
  }
  .note {
    margin: 0 0 0.8rem -1.5rem;
    padding: 0.6rem 0.9rem;
    border-left: 3px solid var(--accent);
    background: var(--accent-soft);
    border-radius: 0 10px 10px 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-top: 1.5rem;
  }
  pre {
    white-space: pre-wrap;
    overflow-x: auto;
    font-size: 0.75rem;
  }
  .nutrition {
    padding: 0.75rem;
    white-space: pre;
  }
  .problem {
    padding: 1rem;
  }
  .toast {
    position: fixed;
    left: 50%;
    bottom: calc(1.5rem + env(safe-area-inset-bottom));
    transform: translateX(-50%);
    background: var(--text);
    color: var(--bg);
    padding: 0.6rem 1rem;
    border-radius: 999px;
    font-size: 0.9rem;
    z-index: 30;
  }
</style>
