// Matching typed names against the food list (aliases.yaml), the way the
// server does, so an item added offline already shows in the right aisle.

import type { Food } from './types';

/** Lowercase, trimmed, single spaces: like `normalize_name` on the server. */
export function normalize(name: string): string {
  return name.trim().split(/\s+/).join(' ').toLowerCase();
}

/** The food a name means (its own name or a synonym, also plural). */
export function resolveFood(foods: readonly Food[], name: string): Food | undefined {
  const key = normalize(name);
  if (!key) return undefined;
  const candidates = [key, key.endsWith('es') ? key.slice(0, -2) : null, key.endsWith('s') ? key.slice(0, -1) : null];
  for (const c of candidates) {
    if (!c) continue;
    const hit = foods.find((f) => normalize(f.name) === c || f.synonyms.some((s) => normalize(s) === c));
    if (hit) return hit;
  }
  return undefined;
}

export interface Suggestion {
  food: Food;
  /** The name or synonym that matched what was typed. */
  matched: string;
}

/**
 * Foods for what's being typed: names starting with it first, then names
 * containing it. One entry per food.
 */
export function suggestFoods(foods: readonly Food[], query: string, limit = 6): Suggestion[] {
  const q = normalize(query);
  if (!q) return [];
  const scored: { s: Suggestion; score: number }[] = [];
  for (const food of foods) {
    let best: { matched: string; score: number } | undefined;
    for (const name of [food.name, ...food.synonyms]) {
      const n = normalize(name);
      const score = n === q ? 0 : n.startsWith(q) ? 1 : n.split(' ').some((w) => w.startsWith(q)) ? 2 : n.includes(q) ? 3 : -1;
      if (score >= 0 && (!best || score < best.score)) best = { matched: name, score };
    }
    if (best) scored.push({ s: { food, matched: best.matched }, score: best.score });
  }
  scored.sort((a, b) => a.score - b.score || a.s.matched.length - b.s.matched.length);
  return scored.slice(0, limit).map((x) => x.s);
}

/** Units recognised in typed text ("500 g kaas", "2 blik tomaten"). */
const UNITS = new Set(
  (
    'g gr gram kg kilo mg ons ml cl dl l liter el tl eetlepel eetlepels theelepel theelepels ' +
    'kop kopje kopjes stuk stuks blik blikje blikjes blikken bos bosje bosjes zak zakje zakjes ' +
    'pak pakje pakjes fles flessen pot potje potjes teen teentje teentjes tenen plak plakken ' +
    'bakje bakjes doos doosje krop kroppen bol bollen snufje rol rollen net netje'
  ).split(' '),
);

/** "2", "2.5", "2,5", "1/2", "½"; `undefined` for anything else. */
export function parseNumber(token: string): number | undefined {
  const t = token.trim();
  const vulgar: Record<string, number> = { '½': 0.5, '¼': 0.25, '¾': 0.75, '⅓': 1 / 3, '⅔': 2 / 3 };
  if (t in vulgar) return vulgar[t];
  const frac = /^(\d+)\/(\d+)$/.exec(t);
  if (frac) return Number(frac[2]) ? Number(frac[1]) / Number(frac[2]) : undefined;
  return /^\d+([.,]\d+)?$/.test(t) ? Number(t.replace(',', '.')) : undefined;
}

export interface Typed {
  name: string;
  amount?: number;
  /** Only with `amount`; none means pieces. */
  unit?: string;
}

/**
 * Split an amount off a typed item, before or after the name: "500 g kaas",
 * "500g kaas", "2 bananen", "kaas 500 g". A word only counts as a unit when
 * it's a known one ("2 bananen" is two of "bananen"), and something must be
 * left for the name.
 */
export function parseTyped(text: string): Typed {
  const words = text.trim().split(/\s+/).filter(Boolean);
  const whole: Typed = { name: words.join(' ') };
  // A number, maybe with a unit stuck to it ("500g").
  const amountOf = (word: string): { amount: number; unit?: string } | undefined => {
    const m = /^(\d+(?:[.,]\d+)?|\d+\/\d+|[½¼¾⅓⅔])([a-z]+)?$/i.exec(word);
    if (!m) return undefined;
    const amount = parseNumber(m[1]);
    if (amount === undefined || amount <= 0) return undefined;
    if (m[2] && !UNITS.has(m[2].toLowerCase())) return undefined;
    return { amount, unit: m[2]?.toLowerCase() };
  };
  const isUnit = (word: string | undefined) => !!word && UNITS.has(word.toLowerCase());
  const typed = (a: { amount: number; unit?: string }, unit: string | undefined, rest: string[]): Typed =>
    rest.length ? { name: rest.join(' '), amount: a.amount, ...((a.unit ?? unit) ? { unit: a.unit ?? unit } : {}) } : whole;

  const first = words.length > 1 ? amountOf(words[0]) : undefined;
  if (first) {
    return !first.unit && isUnit(words[1]) && words.length > 2
      ? typed(first, words[1].toLowerCase(), words.slice(2))
      : typed(first, undefined, words.slice(1));
  }
  const last = words.length > 1 ? amountOf(words[words.length - 1]) : undefined;
  if (last) return typed(last, undefined, words.slice(0, -1));
  if (words.length > 2 && isUnit(words[words.length - 1])) {
    const before = amountOf(words[words.length - 2]);
    if (before && !before.unit) return typed(before, words[words.length - 1].toLowerCase(), words.slice(0, -2));
  }
  return whole;
}

/** "500 g", "2", "1.5 l": how an amount reads until the server formats it. */
export function amountText(amount: number, unit?: string | null): string {
  const n = Number.isInteger(amount) ? String(amount) : String(Math.round(amount * 100) / 100);
  return unit ? `${n} ${unit}` : n;
}
