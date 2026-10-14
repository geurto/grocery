<script lang="ts">
  let {
    value = $bindable(),
    min = 1,
    max = 99,
    step = 1,
    label,
    onchange,
  }: {
    value: number;
    min?: number;
    max?: number;
    step?: number;
    label: string;
    /** Called after a tap, with the new value. */
    onchange?: (value: number) => void;
  } = $props();

  function set(v: number) {
    // Round away float noise: 0.1 + 0.2 steps.
    value = Math.round(Math.min(max, Math.max(min, v)) * 100) / 100;
    onchange?.(value);
  }

  const shown = $derived(value === 0.5 ? '½' : Number.isInteger(value) ? String(value) : value.toString().replace(/\.5$/, '½'));
</script>

<div class="stepper" role="group" aria-label={label}>
  <button type="button" onclick={() => set(value - step)} disabled={value <= min} aria-label="Fewer {label}">−</button>
  <output aria-live="polite" aria-label="{value} {label}">{shown}</output>
  <button type="button" onclick={() => set(value + step)} disabled={value >= max} aria-label="More {label}">+</button>
</div>

<style>
  .stepper {
    display: inline-flex;
    align-items: center;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--surface);
  }
  button {
    width: 44px;
    height: 40px;
    border: 0;
    background: none;
    font-size: 1.3rem;
    line-height: 1;
  }
  button:disabled {
    opacity: 0.35;
  }
  output {
    min-width: 2ch;
    text-align: center;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }
</style>
