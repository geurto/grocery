import { describe, expect, it } from 'vitest';
import { applyOps, joined, newItem, next, remap, settle, type Op, type Outcome, type SyncState } from './sync';
import type { List } from './types';

function list(id = 1): List {
  return {
    id,
    title: 'Week 42',
    created: '2026-10-14T10:00:00+02:00',
    status: 'open',
    item_count: 2,
    checked_count: 0,
    recipes: [],
    pantry: [],
    items: [
      { ...newItem(10, 'onion'), aisle: 'groente', quantity: '2' },
      { ...newItem(11, 'garlic'), aisle: 'groente', quantity: '3 cloves' },
    ],
  };
}

describe('applyOps', () => {
  it('shows pending changes on top of the server list', () => {
    const ops: Op[] = [
      { seq: 1, kind: 'check', list: 1, item: 10, checked: true },
      { seq: 2, kind: 'add', list: 1, temp: -5, name: 'toilet paper' },
      { seq: 3, kind: 'check', list: 1, item: -5, checked: true },
      { seq: 4, kind: 'remove', list: 1, item: 11 },
      { seq: 5, kind: 'check', list: 2, item: 10, checked: false }, // other list
    ];
    const view = applyOps(list(), ops);
    expect(view.items.map((i) => [i.id, i.name, i.checked])).toEqual([
      [10, 'onion', true],
      [-5, 'toilet paper', true],
    ]);
    expect(view.item_count).toBe(2);
    expect(view.checked_count).toBe(2);
  });

  it('the last tap wins', () => {
    const ops: Op[] = [
      { seq: 1, kind: 'check', list: 1, item: 10, checked: true },
      { seq: 2, kind: 'check', list: 1, item: 10, checked: false },
    ];
    expect(applyOps(list(), ops).items[0].checked).toBe(false);
  });

  it('leaves the list alone without ops', () => {
    const l = list();
    expect(applyOps(l, [])).toBe(l);
  });

  it("doesn't duplicate an add the server already has", () => {
    const l = list();
    l.items.push(newItem(-5, 'toilet paper'));
    const view = applyOps(l, [{ seq: 1, kind: 'add', list: 1, temp: -5, name: 'toilet paper' }]);
    expect(view.items).toHaveLength(3);
  });

  it('shows the amount of an item added with one', () => {
    const view = applyOps(list(), [
      { seq: 1, kind: 'add', list: 1, temp: -5, name: 'kaas', amount: 500, unit: 'g' },
      { seq: 2, kind: 'add', list: 1, temp: -6, name: 'bananen', amount: 2 },
    ]);
    expect(view.items.slice(-2).map((i) => [i.name, i.quantity])).toEqual([
      ['kaas', '500 g'],
      ['bananen', '2'],
    ]);
  });
});

describe('remap', () => {
  it('rewrites temporary ids', () => {
    const ops: Op[] = [
      { seq: 1, kind: 'check', list: 1, item: -5, checked: true },
      { seq: 2, kind: 'remove', list: 1, item: 10 },
    ];
    expect(remap(ops, new Map([[-5, 42]]))).toEqual([
      { seq: 1, kind: 'check', list: 1, item: 42, checked: true },
      { seq: 2, kind: 'remove', list: 1, item: 10 },
    ]);
  });
});

describe('next and settle', () => {
  const ok: Outcome = { ok: true };

  it('folds sent ops into the cached list and maps new ids', () => {
    let state: SyncState = {
      lists: { 1: list() },
      outbox: [
        { seq: 1, kind: 'add', list: 1, temp: -5, name: 'toilet paper' },
        { seq: 2, kind: 'check', list: 1, item: -5, checked: true },
      ],
    };
    const first = next(state);
    expect(first.op?.seq).toBe(1);
    state = settle(first.state, first.op!, { ok: true, id: 42 });
    expect(state.outbox).toEqual([{ seq: 2, kind: 'check', list: 1, item: 42, checked: true }]);
    expect(state.lists[1].items.at(-1)).toMatchObject({ id: 42, name: 'toilet paper' });

    const second = next(state);
    state = settle(second.state, second.op!, ok);
    expect(state.outbox).toEqual([]);
    expect(state.lists[1].items.at(-1)).toMatchObject({ id: 42, checked: true });
    // Once the refetch brings the real item, nothing is shown twice.
    expect(applyOps(state.lists[1], state.outbox).items).toHaveLength(3);
  });

  it('keeps an op to retry at the head', () => {
    const state: SyncState = {
      lists: { 1: list() },
      outbox: [{ seq: 1, kind: 'check', list: 1, item: 10, checked: true }],
    };
    const after = settle(state, state.outbox[0], { ok: false, retry: true });
    expect(after).toBe(state);
  });

  it('drops refused ops, and ops on items that were never created', () => {
    let state: SyncState = {
      lists: {},
      outbox: [
        { seq: 1, kind: 'add', list: 9, temp: -5, name: 'x' }, // list deleted elsewhere
        { seq: 2, kind: 'check', list: 9, item: -5, checked: true },
        { seq: 3, kind: 'check', list: 1, item: 10, checked: true },
      ],
    };
    state = settle(state, state.outbox[0], { ok: false, retry: false });
    const n = next(state);
    expect(n.op?.seq).toBe(3);
    expect(n.state.outbox).toHaveLength(1);
  });

  it('refuses to settle anything but the head', () => {
    const state: SyncState = {
      lists: {},
      outbox: [
        { seq: 1, kind: 'check', list: 1, item: 10, checked: true },
        { seq: 2, kind: 'check', list: 1, item: 11, checked: true },
      ],
    };
    expect(() => settle(state, state.outbox[1], { ok: true })).toThrow();
  });
});

describe('joining a line', () => {
  it('adds up the same unit, and unticks', () => {
    const view = applyOps({ ...list(), items: [{ ...newItem(10, 'limoen'), quantity: '2', checked: true }] }, [
      { seq: 1, kind: 'add', list: 1, temp: -5, name: 'limoen', amount: 12, into: 10 },
    ]);
    expect(view.items.map((i) => [i.id, i.quantity, i.checked])).toEqual([[10, '14', false]]);
  });

  it('leaves amounts it can’t add for the server', () => {
    expect(joined({ ...newItem(1, 'kaas'), quantity: '1.25 kg' }, 500, 'g').quantity).toBe('1.25 kg');
    expect(joined({ ...newItem(1, 'kaas'), quantity: '250 g' }, 500, 'g').quantity).toBe('750 g');
    expect(joined(newItem(1, 'kaas'), 500, 'g').quantity).toBe('500 g');
  });

  it('follows a line added offline to its real id', () => {
    let state: SyncState = {
      lists: { 1: list() },
      outbox: [
        { seq: 1, kind: 'add', list: 1, temp: -5, name: 'limoen', amount: 2 },
        { seq: 2, kind: 'add', list: 1, temp: -6, name: 'limoen', amount: 12, into: -5 },
      ],
    };
    state = settle(state, state.outbox[0], { ok: true, id: 42 });
    expect(state.outbox[0]).toMatchObject({ temp: -6, into: 42 });
    state = settle(state, state.outbox[0], { ok: true, id: 42 });
    const limes = state.lists[1].items.filter((i) => i.name === 'limoen');
    expect(limes.map((i) => [i.id, i.quantity])).toEqual([[42, '14']]);
  });
});
