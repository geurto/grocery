import { describe, expect, it } from 'vitest';
import { byAisle, itemText, plainText } from './format';
import { newItem } from './sync';
import type { List } from './types';

const item = (id: number, name: string, aisle: string | null, checked = false) => ({
  ...newItem(id, name),
  aisle,
  checked,
});

describe('format', () => {
  it('groups by aisle in server order, unticked first', () => {
    const groups = byAisle([
      item(1, 'basil', 'groente & fruit', true),
      item(2, 'onion', 'groente & fruit'),
      item(3, 'feta', 'kaas'),
      item(4, 'toilet paper', null),
    ]);
    expect(groups.map((g) => [g.aisle, g.items.map((i) => i.name)])).toEqual([
      ['Groente & fruit', ['onion', 'basil']],
      ['Kaas', ['feta']],
      ['Other', ['toilet paper']],
    ]);
  });

  it('follows the walking order when known, for items added offline', () => {
    const items = [item(1, 'toilet paper', null), item(2, 'feta', 'kaas'), item(3, 'onion', 'Groente'), item(4, 'x', 'zz')];
    expect(byAisle(items, ['groente', 'kaas']).map((g) => g.aisle)).toEqual(['Groente', 'Kaas', 'Zz', 'Other']);
    // Without it: server order, "Other" last.
    expect(byAisle(items).map((g) => g.aisle)).toEqual(['Kaas', 'Groente', 'Zz', 'Other']);
  });

  it('formats items like the server does', () => {
    expect(itemText({ ...item(1, 'garlic', null), quantity: '4 cloves' })).toBe('garlic — 4 cloves');
    expect(itemText({ ...item(1, 'basil', null), notes: ['a handful'] })).toBe('basil — a handful');
    expect(
      itemText({ ...item(1, 'parmesan', null), quantity: '30 g', notes: ['grated'], optional: true }),
    ).toBe('parmesan — 30 g, grated (optional)');
  });

  it('plain text leaves out ticked items', () => {
    const list = {
      items: [
        { ...item(1, 'onion', 'groente'), quantity: '2' },
        item(2, 'basil', 'groente', true),
        { ...item(3, 'parmesan', null), quantity: '50 g', optional: true },
      ],
    } as List;
    // Same as render::plain in grocery-core.
    expect(plainText(list)).toBe('Groente\n• onion — 2\n\nOther\n• parmesan — 50 g (optional)');
  });
});
