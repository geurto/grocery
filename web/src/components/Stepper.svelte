<script lang="ts">
  let {
    value = $bindable(),
    min = 1,
    max = 99,
    label,
  }: { value: number; min?: number; max?: number; label: string } = $props();
</script>

<div class="stepper" role="group" aria-label={label}>
  <button type="button" onclick={() => (value = Math.max(min, value - 1))} disabled={value <= min} aria-label="Fewer {label}">−</button>
  <output aria-live="polite">{value}</output>
  <button type="button" onclick={() => (value = Math.min(max, value + 1))} disabled={value >= max} aria-label="More {label}">+</button>
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
