<script lang="ts">
  import { tick, untrack } from 'svelte';
  import Header from '../components/Header.svelte';
  import StepText from '../components/StepText.svelte';
  import Timers from '../components/Timers.svelte';
  import { allSteps, current, ingredientName, timersLeft } from '../lib/cooking';
  import { cooking } from '../lib/cooking.svelte';
  import { wakeLock } from '../lib/device';
  import { kitchen } from '../lib/kitchen.svelte';
  import { durationRange } from '../lib/recipes';
  import { router } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';
  import type { Recipe, Seconds } from '../lib/types';

  let { path, servings }: { path: string; servings?: number } = $props();

  let recipe = $state<Recipe | undefined>(untrack(() => store.cachedRecipe(path, servings)));
  let missing = $state(false);

  $effect(() => {
    const [p, s] = [path, servings];
    untrack(() => {
      store.recipe(p, s).then(
        (r) => {
          if (r) recipe = r;
          else if (!recipe) missing = true;
        },
        () => {
          if (!recipe) missing = true;
        },
      );
      if (!cooking.get(p)) cooking.start(p, s);
    });
  });

  // Keep the screen on while cooking.
  $effect(() => {
    void wakeLock.set(true);
    return () => void wakeLock.set(false);
  });

  const progress = $derived(cooking.get(path));
  const done = $derived(new Set(progress?.steps ?? []));
  const prepped = $derived(new Set(progress?.prep ?? []));
  const steps = $derived(recipe?.display ? allSteps(recipe.display) : []);
  const now = $derived(current(steps, done));
  const left = $derived(timersLeft(steps, done));
  const ingredients = $derived(recipe?.display?.ingredients ?? []);
  const allPrepped = $derived(ingredients.every((l) => prepped.has(ingredientName(l))));
  let prepOpen = $state(untrack(() => (cooking.get(path)?.steps.length ?? 0) === 0));

  /** "Ready around 18:40", from the time the recipe states. */
  const readyAt = $derived.by(() => {
    const minutes = recipe?.display?.time?.stated_minutes;
    if (!minutes || !progress) return null;
    const at = new Date(progress.started + minutes * 60_000);
    return at.getTime() > kitchen.now ? at.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : null;
  });

  async function finishStep(text: string) {
    const wasDone = done.has(text);
    cooking.toggleStep(path, text);
    if (wasDone) return;
    prepOpen = false;
    await tick();
    document.querySelector('.step.current')?.scrollIntoView({ behavior: 'smooth', block: 'center' });
  }

  function startTimer(stepNo: number, timer: { subject: string | null; text: string; seconds: Seconds }) {
    const what = timer.subject ? `${stepNo + 1}. ${timer.subject}` : `Step ${stepNo + 1}`;
    kitchen.start(
      { label: `${what} · ${durationRange(timer.seconds)}`, path, servings },
      // For a range, ring at the short end: better to check early.
      timer.seconds.min,
    );
  }

  function finish() {
    cooking.clear(path);
    router.go({ name: 'recipe', path, servings }, true);
  }

  function startOver() {
    cooking.clear(path);
    cooking.start(path, servings);
    prepOpen = true;
    scrollTo({ top: 0, behavior: 'smooth' });
  }
</script>

<Header title={recipe?.title ?? 'Cooking'} back />

<div class="timer-bar">
  <Timers />
</div>

<main>
  {#if !recipe}
    <p class="empty">{missing ? 'This recipe isn’t on this device yet, and the server can’t be reached.' : 'Loading…'}</p>
  {:else if recipe.error || !recipe.display}
    <p class="empty">This recipe doesn’t parse; fix the .cook file.</p>
  {:else}
    <p class="summary">
      <strong>{now ? `Step ${now.n + 1} of ${steps.length}` : 'All steps done'}</strong>
      {#if left}<span class="muted"> · {durationRange(left)} in timers to go</span>{/if}
      {#if readyAt}<span class="muted"> · ready around {readyAt}</span>{/if}
    </p>

    <details class="card prep" bind:open={prepOpen}>
      <summary>
        <span>Set out</span>
        <span class="muted small">{ingredients.filter((l) => prepped.has(ingredientName(l))).length}/{ingredients.length}{allPrepped ? ' ✓' : ''}</span>
      </summary>
      <ul>
        {#each ingredients as line (line)}
          {@const name = ingredientName(line)}
          <li>
            <label class:on={prepped.has(name)}>
              <input type="checkbox" checked={prepped.has(name)} onchange={() => cooking.togglePrep(path, name)} />
              <span>{line}</span>
            </label>
          </li>
        {/each}
      </ul>
    </details>

    <ol class="steps">
      {#each steps as step (step.n)}
        {@const isDone = done.has(step.text)}
        {#if step.section && (step.n === 0 || steps[step.n - 1].section !== step.section)}
          <li class="section"><h2>{step.section}</h2></li>
        {/if}
        <li class="step card" class:done={isDone} class:current={now?.n === step.n}>
          <button
            class="check"
            onclick={() => finishStep(step.text)}
            aria-pressed={isDone}
            aria-label={isDone ? `Step ${step.n + 1} done; tap to undo` : `Mark step ${step.n + 1} done`}
          >
            {isDone ? '✓' : step.n + 1}
          </button>
          <div class="text">
            <StepText {step} ontimer={isDone ? undefined : (t) => startTimer(step.n, t)} />
          </div>
        </li>
      {/each}
    </ol>

    <div class="actions">
      {#if !now}
        <button class="btn primary" onclick={finish}>Finish cooking</button>
      {/if}
      <button class="btn" onclick={startOver}>Start over</button>
      {#if now}<button class="btn" onclick={finish}>Stop cooking</button>{/if}
    </div>
  {/if}
</main>

<style>
  .timer-bar {
    position: sticky;
    top: calc(57px + env(safe-area-inset-top));
    z-index: 9;
    max-width: 40rem;
    margin: 0 auto;
    padding: 0 max(1rem, env(safe-area-inset-left));
  }
  .timer-bar :global(.timers) {
    padding: 0.5rem 0;
    background: var(--bg);
  }
  .summary {
    margin: 0.25rem 0.25rem 0.75rem;
  }
  .prep summary {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.8rem 1rem;
    font-weight: 650;
    cursor: pointer;
    min-height: var(--tap);
  }
  .prep ul {
    list-style: none;
    margin: 0;
    padding: 0 0 0.25rem;
  }
  .prep li {
    border-top: 1px solid var(--line);
  }
  .prep label {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.6rem 1rem;
    min-height: 48px;
    cursor: pointer;
  }
  .prep input {
    width: 22px;
    height: 22px;
    accent-color: var(--accent);
    flex: none;
  }
  .prep label.on span {
    color: var(--muted);
    text-decoration: line-through;
  }
  .steps {
    list-style: none;
    margin: 1rem 0 0;
    padding: 0;
    display: grid;
    gap: 0.75rem;
  }
  .section h2 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 0.75rem 0 0 0.25rem;
  }
  .step {
    display: flex;
    gap: 0.9rem;
    padding: 1rem;
    font-size: 1.1rem;
    line-height: 1.65;
    transition: opacity 0.2s;
  }
  .step.current {
    border: 2px solid var(--accent);
    font-size: 1.25rem;
    box-shadow: 0 4px 18px rgb(0 0 0 / 0.08);
  }
  .step.done {
    opacity: 0.55;
    font-size: 1rem;
  }
  .step.done .text {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .check {
    flex: none;
    width: 48px;
    height: 48px;
    border-radius: 50%;
    border: 2px solid var(--line);
    background: var(--surface);
    font-weight: 700;
    font-size: 1.05rem;
    color: var(--muted);
  }
  .current .check {
    border-color: var(--accent);
    color: var(--accent);
  }
  .done .check {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  .text {
    flex: 1;
    min-width: 0;
    padding-top: 0.45rem;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-top: 1.5rem;
  }
</style>
