<script lang="ts">
  import { durationRange, stepItems, timerSubject } from '../lib/recipes';
  import type { Seconds, Step } from '../lib/types';

  let {
    step,
    ontimer,
  }: {
    step: Extract<Step, { type: 'step' }>;
    /** Makes timers tappable. */
    ontimer?: (timer: { subject: string | null; text: string; seconds: Seconds }) => void;
  } = $props();
</script>

{#each stepItems(step) as item, i (i)}
  {#if item.type === 'ingredient'}
    <span class="ingredient">{item.name}{#if item.amount}<span class="amount">{item.amount}</span>{/if}</span>
  {:else if item.type === 'timer'}
    {@const seconds = item.seconds}
    {#if seconds && ontimer}
      <button
        type="button"
        class="timer"
        onclick={() => ontimer({ subject: timerSubject(stepItems(step), i), text: item.text, seconds })}
        aria-label="Start a timer for {item.text}"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="13" r="8" /><path d="M12 9v4l2 2M10 2h4" /></svg>
        {durationRange(seconds)}
      </button>
    {:else}
      <span class="timer">
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="13" r="8" /><path d="M12 9v4l2 2M10 2h4" /></svg>
        {seconds ? durationRange(seconds) : item.text}
      </span>
    {/if}
  {:else if item.type === 'cookware'}
    <span class="cookware">{item.name}</span>
  {:else}
    {item.text}
  {/if}
{/each}

<style>
  .ingredient {
    font-weight: 650;
  }
  .amount {
    margin-left: 0.3em;
    padding: 0 0.35em;
    border-radius: 6px;
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
    font-size: 0.9em;
    white-space: nowrap;
  }
  .timer {
    display: inline-flex;
    align-items: center;
    gap: 0.25em;
    padding: 0.05em 0.5em;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--surface-2);
    font: inherit;
    font-size: 0.9em;
    font-weight: 600;
    white-space: nowrap;
    vertical-align: baseline;
  }
  button.timer {
    border-color: var(--accent);
    color: var(--accent);
    background: var(--accent-soft);
  }
  .timer svg {
    width: 1em;
    height: 1em;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
  }
</style>
