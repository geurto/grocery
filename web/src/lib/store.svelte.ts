// App state: what the server last told us (cached on the phone, so the app
// works in a shop without signal), the outbox of changes still to send, and
// the live connection that tells us when someone else changed something.

import { api, HttpError, OfflineError } from './api';
import { load, save } from './storage';
import { applyOps, next, settle, type Op, type Outcome } from './sync';
import type { List, ListStatus, ListSummary, Recipe, RecipeSummary, ServerEvent } from './types';

const RECIPE_CACHE_SIZE = 40;
const RETRY_MS = 15_000;

const recipeKey = (path: string, servings?: number) => `${path}|${servings ?? ''}`;

class Store {
  lists = $state<ListSummary[]>(load('lists', []));
  /** Server state per list (with sent changes folded in). */
  details = $state<Record<number, List>>(load('details', {}));
  recipes = $state<RecipeSummary[]>(load('recipes', []));
  outbox = $state<Op[]>(load('outbox', []));
  /** Whether the last request reached the server. */
  online = $state(true);
  /** Whether live updates are connected. */
  live = $state(false);
  /** A message for the user, shown as a toast. */
  notice = $state<string | null>(null);

  #recipeCache: Record<string, Recipe> = load('recipeCache', {});
  #seq = Math.max(0, ...this.outbox.map((o) => o.seq));
  #flushing = false;
  #retryTimer: ReturnType<typeof setTimeout> | undefined;
  /** Bumped whenever a sent change is folded into a list. */
  #versions = new Map<number, number>();
  #refreshing = new Map<number, Promise<void>>();
  #events: EventSource | undefined;
  #listeners = new Set<(event: ServerEvent) => void>();

  /** Hear about server events (and `resync` when everything should be refetched). */
  on(listener: (event: ServerEvent) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  #emit(event: ServerEvent) {
    for (const l of this.#listeners) l(event);
  }

  // ------------------------------------------------------------ reading

  /** A list as it will be once pending changes are through. */
  list(id: number): List | undefined {
    const l = this.details[id];
    return l && applyOps(l, this.outbox);
  }

  /** Overview rows, with counts from pending changes where we know them. */
  get summaries(): ListSummary[] {
    return this.lists.map((s) => {
      const l = this.list(s.id);
      return l ? { ...s, item_count: l.item_count, checked_count: l.checked_count } : s;
    });
  }

  pending(listId?: number): number {
    return this.outbox.filter((o) => listId === undefined || o.list === listId).length;
  }

  // ------------------------------------------------------------ fetching

  /** Run a request; track whether we're online. Returns undefined when offline. */
  async #try<T>(fn: () => Promise<T>): Promise<T | undefined> {
    try {
      const value = await fn();
      this.online = true;
      return value;
    } catch (e) {
      if (e instanceof OfflineError) {
        this.online = false;
        return undefined;
      }
      throw e;
    }
  }

  async refreshLists() {
    const lists = await this.#try(api.lists);
    if (!lists) return;
    this.lists = lists;
    save('lists', lists);
    // Forget lists deleted elsewhere (unless changes for them are pending).
    const known = new Set(lists.map((l) => l.id));
    let changed = false;
    for (const id of Object.keys(this.details).map(Number)) {
      if (!known.has(id) && this.pending(id) === 0) {
        delete this.details[id];
        changed = true;
      }
    }
    if (changed) save('details', this.details);
  }

  /** Fetch one list. Concurrent calls share one request. */
  refreshList(id: number): Promise<void> {
    const running = this.#refreshing.get(id);
    if (running) return running;
    const run = this.#fetchList(id)
      .catch((e) => {
        this.notice = `Couldn't load the list: ${e instanceof Error ? e.message : e}`;
      })
      .finally(() => this.#refreshing.delete(id));
    this.#refreshing.set(id, run);
    return run;
  }

  async #fetchList(id: number) {
    let list: List | undefined;
    for (;;) {
      const version = this.#versions.get(id) ?? 0;
      try {
        list = await this.#try(() => api.list(id));
      } catch (e) {
        if (e instanceof HttpError && e.status === 404) {
          delete this.details[id];
          this.lists = this.lists.filter((l) => l.id !== id);
          save('details', this.details);
          save('lists', this.lists);
          return;
        }
        throw e;
      }
      if (!list) return;
      // If one of our changes landed while this was in flight, the answer
      // may predate it: ask again rather than show a stale list.
      if ((this.#versions.get(id) ?? 0) === version) break;
    }
    this.details[id] = list;
    save('details', this.details);
    // Keep the overview row in step.
    const { recipes: _r, items: _i, pantry: _p, warnings: _w, unmapped: _u, ...summary } = list;
    const i = this.lists.findIndex((l) => l.id === id);
    if (i >= 0) this.lists[i] = summary;
    else this.lists = [summary, ...this.lists];
    save('lists', this.lists);
  }

  async refreshRecipes() {
    const recipes = await this.#try(api.recipes);
    if (!recipes) return;
    this.recipes = recipes;
    save('recipes', recipes);
  }

  /** A recipe from the server, or from the cache when offline. */
  async recipe(path: string, servings?: number): Promise<Recipe | undefined> {
    const key = recipeKey(path, servings);
    const fresh = await this.#try(() => api.recipe(path, servings));
    if (!fresh) return this.#recipeCache[key];
    delete this.#recipeCache[key]; // re-insert as newest
    this.#recipeCache[key] = fresh;
    const keys = Object.keys(this.#recipeCache);
    for (const old of keys.slice(0, Math.max(0, keys.length - RECIPE_CACHE_SIZE))) {
      delete this.#recipeCache[old];
    }
    save('recipeCache', this.#recipeCache);
    return fresh;
  }

  cachedRecipe(path: string, servings?: number): Recipe | undefined {
    return this.#recipeCache[recipeKey(path, servings)];
  }

  // ------------------------------------------------------------ changes

  #enqueue(op: Op) {
    this.outbox.push(op);
    save('outbox', this.outbox);
    void this.flush();
  }

  check(list: number, item: number, checked: boolean) {
    this.#enqueue({ seq: ++this.#seq, kind: 'check', list, item, checked });
  }

  addItem(list: number, name: string) {
    const trimmed = name.trim();
    if (!trimmed) return;
    // Negative and unique on this phone until the server assigns a real id.
    const temp = -(Date.now() * 100 + (this.#seq % 100));
    this.#enqueue({ seq: ++this.#seq, kind: 'add', list, temp, name: trimmed });
  }

  removeItem(list: number, item: number) {
    this.#enqueue({ seq: ++this.#seq, kind: 'remove', list, item });
  }

  async #send(op: Op): Promise<Outcome> {
    try {
      switch (op.kind) {
        case 'check':
          await api.setChecked(op.list, op.item, op.checked);
          return { ok: true };
        case 'add':
          return { ok: true, id: (await api.addItem(op.list, op.name)).id };
        case 'remove':
          await api.removeItem(op.list, op.item);
          return { ok: true };
      }
    } catch (e) {
      if (e instanceof OfflineError) return { ok: false, retry: true };
      if (e instanceof HttpError && e.status >= 500) return { ok: false, retry: true };
      if (e instanceof HttpError && e.status === 404) return { ok: false, retry: false };
      this.notice = `Couldn't save a change: ${e instanceof Error ? e.message : e}`;
      return { ok: false, retry: false };
    }
  }

  /** Send the outbox, oldest first. Safe to call any time. */
  async flush() {
    if (this.#flushing) return;
    this.#flushing = true;
    clearTimeout(this.#retryTimer);
    const touched = new Set<number>();
    try {
      for (;;) {
        const { op, state } = next({ outbox: this.outbox, lists: this.details });
        this.outbox = state.outbox;
        if (!op) break;
        const outcome = await this.#send(op);
        if (!outcome.ok && outcome.retry) {
          this.online = false;
          this.#retryTimer = setTimeout(() => void this.flush(), RETRY_MS);
          break;
        }
        this.online = true;
        // Settle against the current state: taps during the request were
        // appended behind `op`, which is still at the head.
        const after = settle({ outbox: this.outbox, lists: this.details }, op, outcome);
        this.outbox = after.outbox;
        this.details = after.lists;
        this.#versions.set(op.list, (this.#versions.get(op.list) ?? 0) + 1);
        if (outcome.ok) touched.add(op.list);
        save('outbox', this.outbox);
        save('details', this.details);
      }
    } finally {
      this.#flushing = false;
      save('outbox', this.outbox);
    }
    // Without live updates we'd not hear about our own changes: reconcile.
    if (!this.live) for (const id of touched) void this.refreshList(id);
  }

  // These need the server, so they don't work offline.

  async createList(recipes: { path: string; servings?: number }[], title?: string) {
    const list = await api.createList(recipes, title);
    this.online = true;
    this.details[list.id] = list;
    save('details', this.details);
    await this.refreshLists();
    return list;
  }

  async updateList(id: number, change: { title?: string; status?: ListStatus }) {
    await api.updateList(id, change);
    this.online = true;
    await this.refreshList(id);
  }

  async deleteList(id: number) {
    await api.deleteList(id);
    this.online = true;
    this.outbox = this.outbox.filter((o) => o.list !== id);
    delete this.details[id];
    this.lists = this.lists.filter((l) => l.id !== id);
    save('outbox', this.outbox);
    save('details', this.details);
    save('lists', this.lists);
  }

  // ------------------------------------------------------------ live

  /** Refetch everything on screen or cached. */
  async resync() {
    await this.refreshLists();
    await Promise.all(Object.keys(this.details).map((id) => this.refreshList(Number(id))));
    if (this.recipes.length > 0) await this.refreshRecipes();
    this.#emit({ type: 'resync' });
  }

  /** Connect to `/api/events`; EventSource reconnects by itself. */
  connect() {
    if (this.#events) return;
    const events = new EventSource('/api/events');
    this.#events = events;
    events.onopen = () => {
      this.live = true;
      this.online = true;
      // We may have missed events while away.
      void this.flush();
      void this.resync();
    };
    events.onerror = () => {
      this.live = false;
    };
    events.onmessage = (msg) => {
      let event: ServerEvent;
      try {
        event = JSON.parse(msg.data);
      } catch {
        return;
      }
      switch (event.type) {
        case 'list':
          void this.refreshLists();
          if (this.details[event.id]) void this.refreshList(event.id);
          break;
        case 'recipes':
          void this.refreshRecipes();
          break;
        case 'resync':
          void this.resync();
          break;
        default:
          this.#emit(event);
      }
    };

    addEventListener('online', () => {
      void this.flush();
      void this.resync();
    });
    // Phones suspend background tabs; catch up when the app comes back.
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible') {
        void this.flush();
        void this.resync();
      }
    });
  }
}

export const store = new Store();
