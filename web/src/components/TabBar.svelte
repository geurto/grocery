<script lang="ts">
  import { href, router } from '../lib/router.svelte';

  const tabs = [
    { name: 'lists', label: 'Lists', icon: 'M9 6h11M9 12h11M9 18h11M4.5 6h.01M4.5 12h.01M4.5 18h.01' },
    {
      name: 'recipes',
      label: 'Recipes',
      icon: 'M5 4h11a3 3 0 0 1 3 3v13H8a3 3 0 0 1-3-3zM5 17a3 3 0 0 1 3-3h11M9 8h6',
    },
    {
      name: 'plan',
      label: 'Plan',
      icon: 'M4 6a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v13a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2zM4 10h16M8 2v4M16 2v4M8 14h2M14 14h2M8 17h2',
    },
  ] as const;

  const current = $derived(router.route.name);
</script>

<nav aria-label="Sections">
  {#each tabs as tab (tab.name)}
    <a href={href({ name: tab.name })} aria-current={current === tab.name ? 'page' : undefined}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d={tab.icon} /></svg>
      {tab.label}
    </a>
  {/each}
</nav>

<style>
  nav {
    position: fixed;
    z-index: 20;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    justify-content: center;
    background: var(--bg);
    border-top: 1px solid var(--line);
    padding-bottom: env(safe-area-inset-bottom);
  }
  a {
    flex: 1;
    max-width: 10rem;
    height: var(--tabbar);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.15rem;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--muted);
    text-decoration: none;
  }
  a[aria-current='page'] {
    color: var(--accent);
  }
  svg {
    width: 24px;
    height: 24px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
