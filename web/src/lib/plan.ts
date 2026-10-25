import { normalize } from './foods';
import type { PlanEntry } from './types';

export interface PlannedRecipe {
  path: string;
  title: string;
  /** Everyone's portions together: the servings to cook. */
  portions: number;
  /** Who eats how much, for showing. */
  by: { person: number; portions: number }[];
}

/**
 * Recipes to shop for: every planned portion on the chosen days, summed per
 * recipe. Two people each having a portion, or one person having the
 * leftovers the next day, both mean cooking two servings.
 */
export function plannedRecipes(entries: readonly PlanEntry[], days: ReadonlySet<string>): PlannedRecipe[] {
  const out = new Map<string, PlannedRecipe>();
  for (const e of entries) {
    if (!days.has(e.day) || e.food) continue;
    let r = out.get(e.path);
    if (!r) out.set(e.path, (r = { path: e.path, title: e.title, portions: 0, by: [] }));
    r.portions += e.portions;
    const mine = r.by.find((b) => b.person === e.person);
    if (mine) mine.portions += e.portions;
    else r.by.push({ person: e.person, portions: e.portions });
  }
  return [...out.values()];
}

export interface PlannedFood {
  /** As first typed. */
  name: string;
  amount: number;
  unit: string | null;
}

/**
 * Single foods to shop for on the chosen days, summed per name and unit.
 * Different units stay apart; the server adds them up when it can weigh them.
 */
export function plannedFoods(entries: readonly PlanEntry[], days: ReadonlySet<string>): PlannedFood[] {
  const out = new Map<string, PlannedFood>();
  for (const e of entries) {
    if (!days.has(e.day) || !e.food) continue;
    const key = `${normalize(e.title)}|${e.food.unit ?? ''}`;
    const f = out.get(key);
    if (f) f.amount += e.food.qty;
    else out.set(key, { name: e.title, amount: e.food.qty, unit: e.food.unit });
  }
  return [...out.values()];
}
