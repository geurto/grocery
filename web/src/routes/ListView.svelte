<script lang="ts">
  import { untrack } from 'svelte';
  import FoodInfo from '../components/FoodInfo.svelte';
  import Header from '../components/Header.svelte';
  import { api, OfflineError } from '../lib/api';
  import { copyText, shareText, tick, WakeLock, wakeLock } from '../lib/device';
  import { suggestFoods } from '../lib/foods';
  import { byAisle, plainText } from '../lib/format';
  import { href, router } from '../lib/router.svelte';
  import { store } from '../lib/store.svelte';
  import type { Item } from '../lib/types';

  let { id }: { id: number } = $props();

  const UNDO_MS = 4000;

  const KEY = 'grocery:v1:hideTicked';
  let hideTicked = $state(localStorage.getItem(KEY) === '1');
  let keepAwake = $state(false);
  let menuOpen = $state(false);
  let renaming = $state(false);
  let newTitle = $state('');
  let confirmDelete = $state(false);
  let newItem = $state('');
  let toast = $state<string | null>(null);
  let loadFailed = $state(false);
  /** Edit mode: every item gets a remove button. */
  let editing = $state(false);
  /** Removed, but not sent yet: undo is still possible. */
  let removed = $state<Item[]>([]);
  let removeTimer: ReturnType<typeof setTimeout> | undefined;
  /** A food to show nutrition for. */
  let info = $state<string | null>(null);

  const list = $derived(store.list(id));
  const hidden = $derived(new Set(removed.map((i) => i.id)));
  const visible = $derived(list ? list.items.filter((i) => !hidden.has(i.id)) : []);
  const groups = $derived(
    byAisle(hideTicked ? visible.filter((i) => !i.checked) : visible, store.aisles),
  );
  const suggestions = $derived(suggestFoods(store.foods, newItem));
  const allDone = $derived(!!list && list.item_count > 0 && list.checked_count === list.item_count);

  $effect(() => {
    const listId = id;
    untrack(() => {
      loadFailed = false;
      void store.refreshList(listId).then(() => {
        if (!store.details[listId]) loadFailed = true;
      });
      void store.refreshFoods();
    });
  });

  // Leaving the list sends removals that are still waiting for undo.
  $effect(() => () => commitRemovals());

  function removeItem(item: Item) {
    removed = [...removed, item];
    clearTimeout(removeTimer);
    removeTimer = setTimeout(commitRemovals, UNDO_MS);
  }

  function commitRemovals() {
    clearTimeout(removeTimer);
    for (const item of removed) store.removeItem(id, item.id);
    removed = [];
  }

  function undoRemovals() {
    clearTimeout(removeTimer);
    removed = [];
  }

  $effect(() => {
    localStorage.setItem(KEY, hideTicked ? '1' : '0');
  });

  $effect(() => {
    void wakeLock.set(keepAwake);
    return () => void wakeLock.set(false);
  });

  function flash(message: string) {
    toast = message;
    setTimeout(() => (toast = null), 2500);
  }

  function toggle(itemId: number, checked: boolean) {
    tick();
    store.check(id, itemId, checked);
  }

  function add(e: SubmitEvent) {
    e.preventDefault();
    addNamed(newItem);
  }

  function addNamed(name: string) {
    if (!name.trim()) return;
    store.addItem(id, name);
    newItem = '';
    info = null;
  }

  async function share() {
    menuOpen = false;
    if (!list) return;
    // Built locally so it also works offline.
    const result = await shareText(plainText(list), list.title);
    if (result === 'copied') flash('Copied what’s left to buy');
    if (result === 'failed') flash('Couldn’t share or copy');
  }

  async function copyMarkdown() {
    menuOpen = false;
    try {
      const md = await api.listText(id, 'md');
      flash((await copyText(md)) ? 'Copied as Markdown' : 'Couldn’t copy');
    } catch (e) {
      flash(e instanceof OfflineError ? 'Markdown export needs a connection' : String(e));
    }
  }

  async function run(action: () => Promise<unknown>, offlineMessage: string) {
    try {
      await action();
      return true;
    } catch (e) {
      flash(e instanceof OfflineError ? offlineMessage : e instanceof Error ? e.message : String(e));
      return false;
    }
  }

  async function setStatus(status: 'open' | 'done') {
    menuOpen = false;
    await run(() => store.updateList(id, { status }), 'Needs a connection');
  }

  async function rename(e: SubmitEvent) {
    e.preventDefault();
    const title = newTitle.trim();
    if (!title) return;
    if (await run(() => store.updateList(id, { title }), 'Renaming needs a connection')) {
      renaming = false;
      menuOpen = false;
    }
  }

  async function remove() {
    if (!confirmDelete) {
      confirmDelete = true;
      return;
    }
    if (await run(() => store.deleteList(id), 'Deleting needs a connection')) {
      router.go({ name: 'lists' }, true);
    }
  }
</script>

<Header title={list?.title ?? 'Shopping list'} back>
  {#snippet actions()}
    <button
      class="icon"
      aria-label="List options"
      aria-expanded={menuOpen}
      onclick={() => {
        menuOpen = !menuOpen;
        renaming = false;
        confirmDelete = false;
      }}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"
        ><circle cx="5" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="19" cy="12" r="1.6" /></svg
      >
    </button>
  {/snippet}
</Header>

{#if menuOpen && list}
  <button class="backdrop" tabindex="-1" aria-label="Close menu" onclick={() => (menuOpen = false)}></button>
  <div class="menu card" role="menu">
    <button role="menuitem" onclick={share}>Share what’s left</button>
    <button role="menuitem" onclick={copyMarkdown}>Copy as Markdown</button>
    <label class="switch">
      <input type="checkbox" bind:checked={hideTicked} /> Hide ticked items
    </label>
    {#if WakeLock.supported}
      <label class="switch">
        <input type="checkbox" bind:checked={keepAwake} /> Keep screen on
      </label>
    {/if}
    {#if list.status === 'open'}
      <button role="menuitem" onclick={() => setStatus('done')}>Mark list as done</button>
    {:else}
      <button role="menuitem" onclick={() => setStatus('open')}>Reopen list</button>
    {/if}
    {#if renaming}
      <form class="rename" onsubmit={rename}>
        <input class="field" bind:value={newTitle} aria-label="New title" />
        <button class="btn primary" type="submit">Save</button>
      </form>
    {:else}
      <button
        role="menuitem"
        onclick={() => {
          renaming = true;
          newTitle = list.title;
        }}>Rename</button
      >
    {/if}
    <button role="menuitem" class="danger" onclick={remove}>
      {confirmDelete ? 'Tap again to delete this list' : 'Delete list'}
    </button>
  </div>
{/if}

<main>
  {#if !list}
    <p class="empty">
      {loadFailed ? 'This list isn’t on this phone yet, and the server can’t be reached.' : 'Loading…'}
    </p>
  {:else}
    <div class="summary">
      <div class="progress" aria-hidden="true">
        <span style:width="{list.item_count ? (list.checked_count / list.item_count) * 100 : 0}%"></span>
      </div>
      <p class="small muted">
        {list.checked_count} of {list.item_count} in the basket
        {#if list.recipes.length}
          · for
          {#each list.recipes as r, i (i)}
            {i > 0 ? ', ' : ''}<a href={href({ name: 'recipe', path: r.path, servings: r.servings ?? undefined })}
              >{r.title}</a
            >{/each}
        {/if}
      </p>
    </div>

    <div class="toolbar">
      <a class="btn" href={href({ name: 'list-recipes', id })}>+ Recipes</a>
      {#if list.items.length}
        <button class="btn" class:primary={editing} aria-pressed={editing} onclick={() => (editing = !editing)}>
          {editing ? 'Done' : 'Edit'}
        </button>
      {/if}
    </div>

    {#if list.items.length === 0}
      <div class="empty">
        <p>This list is empty.</p>
        <p class="small">Add recipes, or type what you need below: foods you know land in their aisle.</p>
      </div>
    {/if}

    {#if allDone && list.status === 'open'}
      <div class="done-banner card">
        <span>Everything’s in the basket.</span>
        <button class="btn primary" onclick={() => setStatus('done')}>Mark as done</button>
      </div>
    {/if}

    {#each groups as group (group.aisle)}
      <section>
        <h2>{group.aisle}</h2>
        <ul class="card">
          {#each group.items as item (item.id)}
            <li class:checked={item.checked}>
              <label>
                <input
                  type="checkbox"
                  checked={item.checked}
                  onchange={(e) => toggle(item.id, e.currentTarget.checked)}
                />
                <span class="box" aria-hidden="true">
                  <svg viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>
                </span>
                <span class="text">
                  <span class="name">
                    {item.name}
                    {#if item.optional}<span class="tag">optional</span>{/if}
                  </span>
                  {#if item.notes.length}<span class="notes small muted">{item.notes.join(', ')}</span>{/if}
                  {#if item.recipes.length > 1 || (item.recipes.length === 1 && list.recipes.length > 1)}
                    <span class="recipes small muted">{item.recipes.join(' · ')}</span>
                  {/if}
                </span>
                {#if item.quantity}<span class="qty">{item.quantity}</span>{/if}
              </label>
              {#if editing || item.recipes.length === 0}
                <button class="remove" aria-label="Remove {item.name}" onclick={() => removeItem(item)}>×</button>
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/each}

    {#if hideTicked && list.checked_count > 0}
      <button class="btn show-ticked" onclick={() => (hideTicked = false)}>
        Show {list.checked_count} ticked
      </button>
    {/if}

    {#if list.pantry.length}
      <p class="small muted pantry">From the pantry, not on the list: {list.pantry.join(', ')}</p>
    {/if}
  {/if}
</main>

{#if list}
  {#if suggestions.length}
    <ul class="suggestions card" aria-label="Foods">
      {#each suggestions as s (s.food.name)}
        <li>
          <button class="pick" onclick={() => addNamed(s.matched)}>
            <span class="name">
              {s.matched}{#if s.matched !== s.food.name}<span class="muted"> ({s.food.name})</span>{/if}
            </span>
            <span class="small muted">{s.food.aisle ?? 'no aisle'}</span>
          </button>
          <button class="info" aria-label="Nutrition of {s.food.name}" onclick={() => (info = s.food.name)}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 11v6M12 7.5v.5" /></svg>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  <form class="add" onsubmit={add}>
    <input
      class="field"
      bind:value={newItem}
      placeholder="Add an item…"
      aria-label="Add an item"
      enterkeyhint="done"
      autocomplete="off"
    />
    <button class="btn primary" type="submit" disabled={!newItem.trim()}>Add</button>
  </form>
{/if}

{#if removed.length}
  <div class="toast undo" role="status">
    <span>Removed {removed.length === 1 ? removed[0].name : `${removed.length} items`}</span>
    <button onclick={undoRemovals}>Undo</button>
  </div>
{:else if toast}
  <div class="toast" role="status">{toast}</div>
{/if}

{#if info}
  <FoodInfo name={info} onclose={() => (info = null)} onadd={() => addNamed(info!)} />
{/if}

<style>
  .icon {
    width: 44px;
    height: 44px;
    border: 0;
    background: none;
    border-radius: 50%;
    display: grid;
    place-items: center;
  }
  .icon svg {
    width: 22px;
    height: 22px;
    fill: currentColor;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 19;
    border: 0;
    background: rgb(0 0 0 / 0.15);
  }
  .menu {
    position: fixed;
    z-index: 20;
    top: calc(60px + env(safe-area-inset-top));
    right: max(0.75rem, env(safe-area-inset-right));
    width: min(20rem, calc(100vw - 1.5rem));
    padding: 0.4rem;
    display: grid;
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.2);
  }
  .menu > button,
  .switch {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    min-height: 46px;
    padding: 0 0.75rem;
    border: 0;
    border-radius: 10px;
    background: none;
    text-align: left;
  }
  .menu > button:active,
  .switch:active {
    background: var(--surface-2);
  }
  .switch input {
    width: 20px;
    height: 20px;
    accent-color: var(--accent);
  }
  .danger {
    color: var(--danger);
  }
  .rename {
    display: flex;
    gap: 0.4rem;
    padding: 0.3rem;
  }

  .summary {
    margin: 0.25rem 0 0.75rem;
  }
  .toolbar {
    display: flex;
    gap: 0.5rem;
    margin: 0 0 1rem;
  }
  .toolbar .btn {
    min-height: 40px;
  }
  .suggestions {
    position: fixed;
    z-index: 11;
    left: max(1rem, env(safe-area-inset-left));
    right: max(1rem, env(safe-area-inset-right));
    bottom: calc(4.4rem + env(safe-area-inset-bottom));
    max-width: 38rem;
    margin: 0 auto;
    list-style: none;
    padding: 0;
    overflow: hidden;
    box-shadow: 0 -6px 24px rgb(0 0 0 / 0.14);
  }
  .suggestions li {
    display: flex;
    align-items: center;
  }
  .suggestions li + li {
    border-top: 1px solid var(--line);
  }
  .suggestions .pick {
    flex: 1;
    display: grid;
    text-align: left;
    padding: 0.5rem 0.9rem;
    min-height: 48px;
    border: 0;
    background: none;
  }
  .suggestions .info {
    flex: none;
    width: 48px;
    height: 48px;
    border: 0;
    background: none;
    display: grid;
    place-items: center;
    color: var(--muted);
  }
  .suggestions .info svg {
    width: 22px;
    height: 22px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
  }
  .summary p {
    margin: 0.4rem 0 0;
  }
  .progress {
    height: 6px;
    border-radius: 3px;
    background: var(--surface-2);
    overflow: hidden;
  }
  .progress span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s;
  }
  .done-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0.75rem 0.75rem 0.75rem 1rem;
    margin-bottom: 1rem;
    background: var(--accent-soft);
    border-color: transparent;
  }

  section {
    margin-bottom: 1.1rem;
  }
  h2 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 0 0 0.4rem 0.25rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: hidden;
  }
  li {
    display: flex;
    align-items: center;
    border-top: 1px solid var(--line);
  }
  li:first-child {
    border-top: 0;
  }
  label {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.8rem;
    min-height: var(--tap);
    padding: 0.55rem 0.9rem;
    cursor: pointer;
    user-select: none;
  }
  label input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }
  .box {
    flex: none;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    border: 2px solid var(--line);
    display: grid;
    place-items: center;
    transition:
      background 0.15s,
      border-color 0.15s;
  }
  .box svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: var(--accent-text);
    stroke-width: 3;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0;
  }
  input:focus-visible + .box {
    outline: 3px solid color-mix(in srgb, var(--accent) 55%, transparent);
    outline-offset: 2px;
  }
  .checked .box {
    background: var(--accent);
    border-color: var(--accent);
  }
  .checked .box svg {
    opacity: 1;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: grid;
  }
  .name {
    font-weight: 550;
  }
  .checked .name,
  .checked .qty {
    text-decoration: line-through;
    color: var(--muted);
  }
  .checked .notes,
  .checked .recipes {
    opacity: 0.6;
  }
  .qty {
    flex: none;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    font-weight: 550;
  }
  .tag {
    font-size: 0.7rem;
    font-weight: 600;
    padding: 0.05em 0.45em;
    border-radius: 999px;
    border: 1px solid var(--line);
    color: var(--muted);
    vertical-align: 0.15em;
  }
  .remove {
    flex: none;
    width: 44px;
    height: var(--tap);
    border: 0;
    background: none;
    color: var(--muted);
    font-size: 1.4rem;
  }
  .show-ticked {
    width: 100%;
    margin-bottom: 1rem;
  }
  .pantry {
    margin-top: 1.5rem;
  }

  .add {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    gap: 0.5rem;
    padding: 0.6rem max(1rem, env(safe-area-inset-right)) calc(0.6rem + env(safe-area-inset-bottom))
      max(1rem, env(safe-area-inset-left));
    background: color-mix(in srgb, var(--bg) 92%, transparent);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    border-top: 1px solid var(--line);
  }
  .add .field {
    flex: 1;
  }
  @media (min-width: 42rem) {
    .add {
      left: 50%;
      transform: translateX(-50%);
      width: 40rem;
      border: 1px solid var(--line);
      border-bottom: 0;
      border-radius: var(--radius) var(--radius) 0 0;
    }
  }

  .undo {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding-right: 0.4rem;
  }
  .undo button {
    border: 0;
    background: none;
    color: var(--accent);
    font-weight: 700;
    min-height: 36px;
    padding: 0 0.6rem;
    filter: brightness(1.4);
  }
  .toast {
    position: fixed;
    left: 50%;
    bottom: calc(5.5rem + env(safe-area-inset-bottom));
    transform: translateX(-50%);
    background: var(--text);
    color: var(--bg);
    padding: 0.6rem 1rem;
    border-radius: 999px;
    font-size: 0.9rem;
    z-index: 30;
    max-width: calc(100vw - 2rem);
  }
</style>
