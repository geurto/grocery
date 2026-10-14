<script lang="ts">
  import { store } from '../lib/store.svelte';

  const pending = $derived(store.pending());
  const state = $derived(
    pending > 0 ? 'pending' : !store.online ? 'offline' : store.live ? 'live' : 'connecting',
  );
  const label = $derived(
    {
      pending: `${pending} to sync`,
      offline: 'Offline',
      live: 'Live',
      connecting: 'Connecting',
    }[state],
  );
  const help = $derived(
    {
      pending: 'Your changes are saved on this phone and sent when the connection is back.',
      offline: "Can't reach the server. Ticking off still works and syncs later.",
      live: 'Changes from other phones show up right away.',
      connecting: 'Connecting for live updates…',
    }[state],
  );
</script>

<span class="status {state}" title={help} role="status" aria-live="polite">
  <span class="dot" aria-hidden="true"></span>{label}
</span>

<style>
  .status {
    display: inline-flex;
    align-items: center;
    gap: 0.4em;
    padding: 0.2em 0.65em;
    border-radius: 999px;
    font-size: 0.78rem;
    font-weight: 600;
    white-space: nowrap;
    background: var(--surface-2);
    color: var(--muted);
  }
  .dot {
    width: 0.55em;
    height: 0.55em;
    border-radius: 50%;
    background: currentColor;
  }
  .live {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .pending {
    color: var(--warn);
    background: var(--warn-soft);
  }
</style>
