<script lang="ts">
  import { untrack } from 'svelte';
  import Header from '../components/Header.svelte';
  import { byFolder, searchRecipes } from '../lib/recipes';
  import { href } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';

  let query = $state('');

  $effect(() => {
    untrack(() => void store.refreshRecipes());
  });

  const results = $derived(searchRecipes(store.recipes, query));
  const groups = $derived(byFolder(results));
</script>

<Header title="Recipes" />

<main>
  <input
    class="field search"
    type="search"
    bind:value={query}
    placeholder="Search title, tag or ingredient"
    aria-label="Search recipes"
  />
  {#if store.recipes.length === 0}
    <p class="empty">{store.online ? 'No recipes found in Recipes/.' : 'Recipes load when you’re online.'}</p>
  {:else if results.length === 0}
    <p class="empty">Nothing matches “{query}”.</p>
  {:else}
    {#each groups as group (group.folder)}
      <section>
        {#if group.folder}<h2>{group.folder}</h2>{/if}
        <ul class="card">
          {#each group.recipes as r (r.path)}
            <li>
              <a href={href({ name: 'recipe', path: r.path })}>
                <span class="title">{r.title}</span>
                {#if r.error}
                  <span class="small error">Doesn’t parse; fix the .cook file</span>
                {:else}
                  <span class="small muted">
                    {r.servings ? `serves ${r.servings}` : 'no servings'}{r.tags.length ? ` · ${r.tags.join(', ')}` : ''}
                  </span>
                {/if}
              </a>
            </li>
          {/each}
        </ul>
      </section>
    {/each}
  {/if}
</main>

<style>
  .search {
    margin-bottom: 1rem;
  }
  section {
    margin-bottom: 1.25rem;
  }
  h2 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 0 0 0.5rem 0.25rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: hidden;
  }
  li + li {
    border-top: 1px solid var(--line);
  }
  a {
    display: grid;
    gap: 0.1rem;
    min-height: var(--tap);
    padding: 0.6rem 1rem;
    color: inherit;
    text-decoration: none;
  }
  .title {
    font-weight: 600;
  }
  .error {
    color: var(--danger);
  }
</style>
