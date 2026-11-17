<script lang="ts">
  import { durationRange } from '../lib/recipes';
  import type { Recipe } from '../lib/types';
  import StepText from './StepText.svelte';

  /**
   * Recipes laid out for paper (or "Save as PDF"), one per page. Hidden on
   * screen: only shows up when printing.
   */
  let { recipes }: { recipes: Recipe[] } = $props();
</script>

<div class="print-sheet">
  {#each recipes as recipe (recipe.path)}
    {@const d = recipe.display}
    <article>
      <h1>{recipe.title}</h1>
      <p class="meta">
        {#if recipe.servings}<span>Serves {recipe.servings}</span>{/if}
        {#if d?.time?.stated}
          <span>{d.time.stated}</span>
        {:else if d?.time?.timers}
          <span>{durationRange(d.time.timers)} in timers</span>
        {/if}
        {#if recipe.tags.length}<span>{recipe.tags.join(', ')}</span>{/if}
      </p>
      {#if recipe.description}<p class="description">{recipe.description}</p>{/if}

      {#if d}
        {#if d.ingredients.length}
          <h2>Ingredients</h2>
          <ul class="ingredients">
            {#each d.ingredients as line, i (i)}<li>{line}</li>{/each}
          </ul>
        {/if}

        {#if d.sections.some((s) => s.steps.length)}
          <h2>Method</h2>
          {#each d.sections as section, si (si)}
            {#if section.name}<h3>{section.name}</h3>{/if}
            <ol>
              {#each section.steps as step, i (i)}
                {#if step.type === 'step'}
                  <li><StepText {step} /></li>
                {:else}
                  <p class="note">{step.text}</p>
                {/if}
              {/each}
            </ol>
          {/each}
        {/if}
      {/if}
    </article>
  {/each}
</div>

<style>
  @media screen {
    .print-sheet {
      display: none;
    }
  }
  .print-sheet {
    color: #000;
    background: #fff;
    font-size: 11pt;
    line-height: 1.45;
    /* Paper is light whatever the phone's theme: StepText's chips use these. */
    --line: #bbb;
    --surface-2: #fff;
    --accent: #1f5e39;
    --accent-soft: #fff;
  }
  article {
    break-after: page;
  }
  article:last-child {
    break-after: auto;
  }
  h1 {
    font-size: 20pt;
    margin: 0 0 0.2rem;
  }
  .meta {
    margin: 0 0 0.6rem;
    color: #444;
    font-size: 10pt;
  }
  .meta span + span::before {
    content: ' · ';
  }
  .description {
    margin: 0 0 0.6rem;
    font-style: italic;
  }
  h2 {
    font-size: 10pt;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: #444;
    margin: 1rem 0 0.4rem;
    break-after: avoid;
  }
  h3 {
    font-size: 11pt;
    margin: 0.6rem 0 0.3rem;
    break-after: avoid;
  }
  .ingredients {
    columns: 2;
    column-gap: 2rem;
    margin: 0;
    padding-left: 1.1rem;
  }
  .ingredients li {
    break-inside: avoid;
    margin-bottom: 0.15rem;
  }
  ol {
    margin: 0;
    padding-left: 1.4rem;
  }
  ol li {
    margin-bottom: 0.45rem;
    break-inside: avoid;
  }
  .note {
    margin: 0 0 0.45rem -1.4rem;
    padding-left: 0.6rem;
    border-left: 2px solid #888;
    font-style: italic;
    break-inside: avoid;
  }
</style>
