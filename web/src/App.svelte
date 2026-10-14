<script lang="ts">
  import { router } from './lib/router.svelte';
  import { store } from './lib/store.svelte';
  import ListView from './routes/ListView.svelte';
  import Lists from './routes/Lists.svelte';
  import NewList from './routes/NewList.svelte';
  import Recipes from './routes/Recipes.svelte';
  import RecipeView from './routes/RecipeView.svelte';
  import TabBar from './components/TabBar.svelte';
  import TimerPill from './components/TimerPill.svelte';
  import Cook from './routes/Cook.svelte';
  import Plan from './routes/Plan.svelte';
  import PlanAdd from './routes/PlanAdd.svelte';
  import PlanList from './routes/PlanList.svelte';

  $effect(() => {
    store.connect();
  });

  $effect(() => {
    if (!store.notice) return;
    const timer = setTimeout(() => (store.notice = null), 4000);
    return () => clearTimeout(timer);
  });

  const route = $derived(router.route);
  /** Top-level screens get the tab bar. */
  const tabs = $derived(route.name === 'lists' || route.name === 'recipes' || route.name === 'plan');
</script>

{#if route.name === 'list'}
  {#key route.id}<ListView id={route.id} />{/key}
{:else if route.name === 'new'}
  <NewList />
{:else if route.name === 'cook'}
  {#key route.path}<Cook path={route.path} servings={route.servings} />{/key}
{:else if route.name === 'plan'}
  <Plan day={route.day} />
{:else if route.name === 'plan-add'}
  <PlanAdd day={route.day} meal={route.meal} />
{:else if route.name === 'plan-list'}
  {#key route.start}<PlanList start={route.start} />{/key}
{:else if route.name === 'recipes'}
  <Recipes />
{:else if route.name === 'recipe'}
  {#key route.path}<RecipeView path={route.path} servings={route.servings} />{/key}
{:else}
  <Lists />
{/if}

{#if tabs}
  <TabBar />
{/if}
{#if route.name !== 'cook'}
  <TimerPill above={tabs} />
{/if}

{#if store.notice}
  <div class="notice" role="alert">
    {store.notice}
    <button onclick={() => (store.notice = null)} aria-label="Dismiss">×</button>
  </div>
{/if}

<style>
  .notice {
    position: fixed;
    z-index: 40;
    left: 1rem;
    right: 1rem;
    top: calc(64px + env(safe-area-inset-top));
    max-width: 38rem;
    margin: 0 auto;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.6rem 0.4rem 0.6rem 1rem;
    border-radius: 12px;
    background: var(--danger);
    color: #fff;
    font-size: 0.9rem;
  }
  .notice button {
    margin-left: auto;
    border: 0;
    background: none;
    color: inherit;
    font-size: 1.3rem;
    width: 36px;
    height: 36px;
  }
</style>
