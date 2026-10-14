import type { Item, List } from './types';

/** Heading for items without an aisle (same as the server's exports). */
export const OTHER = 'Other';

export function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/** Items by aisle, in the server's walking order; unticked first within an aisle. */
export function byAisle(items: readonly Item[]): { aisle: string; items: Item[] }[] {
  const groups = new Map<string, Item[]>();
  for (const item of items) {
    const aisle = item.aisle ? capitalize(item.aisle) : OTHER;
    let group = groups.get(aisle);
    if (!group) groups.set(aisle, (group = []));
    group.push(item);
  }
  return [...groups].map(([aisle, items]) => ({
    aisle,
    items: [...items.filter((i) => !i.checked), ...items.filter((i) => i.checked)],
  }));
}

/** "onion — 2, chopped (optional)", as in the server's exports. */
export function itemText(item: Item): string {
  let s = item.name;
  if (item.quantity) s += ` — ${item.quantity}`;
  if (item.notes.length) s += `${item.quantity ? ', ' : ' — '}${item.notes.join(', ')}`;
  if (item.optional) s += ' (optional)';
  return s;
}

/**
 * What's left to buy as plain text, like `?format=txt`. Built here so
 * sharing works offline too.
 */
export function plainText(list: List): string {
  return byAisle(list.items.filter((i) => !i.checked))
    .map((g) => [g.aisle, ...g.items.map((i) => `• ${itemText(i)}`)].join('\n'))
    .join('\n\n');
}
