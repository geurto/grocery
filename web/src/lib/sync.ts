// Offline-first changes to lists.
//
// Every change goes into an outbox first and is shown right away by
// applying the outbox on top of the last list the server sent. The outbox is
// then sent in order, now or whenever the connection comes back. Ticks are
// "set checked to X", not toggles, so sending one twice is harmless and the
// last tap wins, also when two phones tick the same item.
//
// Pure functions only; `store.svelte.ts` does the I/O.

import type { Item, List } from './types';

export type Op =
  | { seq: number; kind: 'check'; list: number; item: number; checked: boolean }
  | {
      seq: number;
      kind: 'add';
      list: number;
      temp: number;
      /** As typed; the server resolves it to a food. */
      name: string;
      /** How it shows until the server answers: the food's name and aisle if known here. */
      shown?: string;
      aisle?: string | null;
    }
  | { seq: number; kind: 'remove'; list: number; item: number };

/** What the server said about one op. */
export type Outcome =
  | { ok: true; id?: number } // `id`: the real id of an added item
  | { ok: false; retry: true } // offline or server trouble: keep it, try later
  | { ok: false; retry: false }; // refused or gone (list deleted): drop it

export function newItem(id: number, name: string, aisle: string | null = null): Item {
  return {
    id,
    name,
    aisle,
    quantity: null,
    notes: [],
    optional: false,
    recipes: [],
    checked: false,
  };
}

/** The list as it will be once the outbox is through. */
export function applyOps(list: List, ops: readonly Op[]): List {
  const mine = ops.filter((op) => op.list === list.id);
  if (mine.length === 0) return list;
  let items = list.items.slice();
  for (const op of mine) {
    switch (op.kind) {
      case 'check':
        items = items.map((i) => (i.id === op.item ? { ...i, checked: op.checked } : i));
        break;
      case 'add':
        if (!items.some((i) => i.id === op.temp)) items.push(newItem(op.temp, op.shown ?? op.name, op.aisle ?? null));
        break;
      case 'remove':
        items = items.filter((i) => i.id !== op.item);
        break;
    }
  }
  return {
    ...list,
    items,
    item_count: items.length,
    checked_count: items.filter((i) => i.checked).length,
  };
}

/** Point ops at real ids once their temporary items reached the server. */
export function remap(ops: readonly Op[], ids: ReadonlyMap<number, number>): Op[] {
  if (ids.size === 0) return ops.slice();
  return ops.map((op) =>
    op.kind !== 'add' && ids.has(op.item) ? { ...op, item: ids.get(op.item)! } : op,
  );
}

export interface SyncState {
  outbox: Op[];
  /** Last known server state of each cached list, with sent ops folded in. */
  lists: Record<number, List>;
}

/**
 * The op to send next, or `undefined` if the outbox is empty. Ops at the
 * head that point at an item that was never created (its add was refused)
 * are dropped on the way.
 */
export function next(state: SyncState): { op: Op | undefined; state: SyncState } {
  let outbox = state.outbox;
  while (outbox.length > 0) {
    const op = outbox[0];
    // A successful add remaps later ops to the real id, so a negative id
    // at the head means the add didn't make it.
    if (op.kind !== 'add' && op.item < 0) {
      outbox = outbox.slice(1);
      continue;
    }
    return { op, state: outbox === state.outbox ? state : { ...state, outbox } };
  }
  return { op: undefined, state: { ...state, outbox } };
}

/**
 * Record what the server said about `op`, the head of the outbox. Sent ops
 * are folded into the cached list at once, so the screen doesn't flicker
 * back if the follow-up refetch fails.
 */
export function settle(state: SyncState, op: Op, outcome: Outcome): SyncState {
  if (state.outbox[0]?.seq !== op.seq) throw new Error('settle: op is not at the head of the outbox');
  if (!outcome.ok && outcome.retry) return state;
  let outbox = state.outbox.slice(1);
  const lists = { ...state.lists };
  if (outcome.ok) {
    let applied: Op = op;
    if (op.kind === 'add' && outcome.id !== undefined) {
      outbox = remap(outbox, new Map([[op.temp, outcome.id]]));
      applied = { ...op, temp: outcome.id };
    }
    const cached = lists[op.list];
    if (cached) lists[op.list] = applyOps(cached, [applied]);
  }
  return { outbox, lists };
}
