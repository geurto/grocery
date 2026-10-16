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
