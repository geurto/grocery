<script lang="ts">
  // Running timers while you're on another screen: the one due first, and a
  // way back to the recipe it belongs to.
  import { kitchen } from '../lib/kitchen.svelte';
  import { href } from '../lib/router.svelte';
  import { clock, remaining } from '../lib/timers';

  let { above = false }: { above?: boolean } = $props();

  const first = $derived([...kitchen.timers].sort((a, b) => a.endsAt - b.endsAt)[0]);
</script>

{#if first}
  {@const ms = remaining(first, kitchen.now)}
  <a
    class="pill"
    class:above
    class:ringing={ms <= 0}
    href={href({ name: 'cook', path: first.path, servings: first.servings })}
  >
    <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="13" r="8" /><path d="M12 9v4l2 2M10 2h4" /></svg>
    <strong>{clock(ms)}</strong>
    <span class="label">{first.label}</span>
    {#if kitchen.timers.length > 1}<span class="more">+{kitchen.timers.length - 1}</span>{/if}
  </a>
{/if}

<style>
  .pill {
    position: fixed;
    z-index: 25;
    left: max(1rem, env(safe-area-inset-left));
    bottom: calc(1rem + env(safe-area-inset-bottom));
    max-width: min(60vw, 18rem);
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-height: 48px;
    padding: 0 1rem;
    border-radius: 999px;
    background: var(--text);
    color: var(--bg);
    text-decoration: none;
    box-shadow: 0 6px 20px rgb(0 0 0 / 0.2);
  }
  .pill.above {
    bottom: calc(1rem + var(--tabbar) + env(safe-area-inset-bottom));
  }
  .pill.ringing {
    background: var(--warn);
  }
  strong {
    font-variant-numeric: tabular-nums;
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.85rem;
    opacity: 0.8;
  }
  .more {
    font-size: 0.8rem;
    opacity: 0.8;
  }
  svg {
    flex: none;
    width: 20px;
    height: 20px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
  }
</style>
