import type { Item, List } from './types';

/** Heading for items without an aisle (same as the server's exports). */
export const OTHER = 'Other';

export function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/**
 * Items by aisle; unticked first within an aisle. Aisles follow `order` (the
 * shop's walking order) when given, else the order the server sent, which
 * matters for items added offline that the server hasn't sorted yet.
 * Aisles not in `order` come after, alphabetically; items without one last.
 */
export function byAisle(items: readonly Item[], order: readonly string[] = []): { aisle: string; items: Item[] }[] {
  const groups = new Map<string, { key: string | null; items: Item[] }>();
  for (const item of items) {
    const aisle = item.aisle ? capitalize(item.aisle) : OTHER;
    let group = groups.get(aisle);
    if (!group) groups.set(aisle, (group = { key: item.aisle, items: [] }));
    group.items.push(item);
  }
  const norm = order.map((a) => a.trim().toLowerCase());
  const rank = (key: string | null): [number, string] => {
    if (key === null) return [Number.MAX_SAFE_INTEGER, ''];
    const i = norm.indexOf(key.trim().toLowerCase());
    return i >= 0 ? [i, ''] : [norm.length, key.toLowerCase()];
  };
  const entries = [...groups];
  if (order.length) {
    entries.sort(([, a], [, b]) => {
      const [ra, na] = rank(a.key);
      const [rb, nb] = rank(b.key);
      return ra - rb || na.localeCompare(nb);
    });
  } else {
    // No walking order known: keep the server's, but "Other" stays last.
    entries.sort(([, a], [, b]) => Number(a.key === null) - Number(b.key === null));
  }
  return entries.map(([aisle, g]) => ({
    aisle,
    items: [...g.items.filter((i) => !i.checked), ...g.items.filter((i) => i.checked)],
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
