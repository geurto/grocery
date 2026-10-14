<script lang="ts">
  import { kitchen } from '../lib/kitchen.svelte';
  import { clock, remaining, ringing } from '../lib/timers';
</script>

{#if kitchen.timers.length}
  <ul class="timers" aria-label="Timers">
    {#each kitchen.timers as t (t.id)}
      {@const ms = remaining(t, kitchen.now)}
      <li class:ringing={ringing(t, kitchen.now)}>
        <span class="clock" role="timer" aria-live={ms <= 0 ? 'assertive' : 'off'}>{clock(ms)}</span>
        <span class="label">{t.label}</span>
        <button class="add" onclick={() => kitchen.add(t.id, 60)} aria-label="One minute more">+1 min</button>
        <button class="stop" onclick={() => kitchen.stop(t.id)} aria-label="Stop timer {t.label}">
          {ms <= 0 ? 'Done' : '×'}
        </button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .timers {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.4rem;
  }
  li {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.35rem 0.35rem 0.35rem 0.9rem;
    border-radius: 999px;
    background: var(--surface);
    border: 1px solid var(--line);
  }
  .clock {
    font-size: 1.3rem;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    min-width: 4.2ch;
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-size: 0.9rem;
  }
  button {
    min-height: 40px;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--surface-2);
    font-weight: 600;
    padding: 0 0.8rem;
  }
  .stop {
    min-width: 44px;
  }
  .ringing {
    background: var(--warn-soft);
    border-color: var(--warn);
    animation: pulse 1s ease-in-out infinite;
  }
  .ringing .clock,
  .ringing .label {
    color: var(--warn);
  }
  .ringing .stop {
    background: var(--warn);
    border-color: var(--warn);
    color: var(--bg);
  }
  @keyframes pulse {
    50% {
      box-shadow: 0 0 0 4px color-mix(in srgb, var(--warn) 35%, transparent);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .ringing {
      animation: none;
    }
  }
</style>
