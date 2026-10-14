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
    if (!days.has(e.day)) continue;
    let r = out.get(e.path);
    if (!r) out.set(e.path, (r = { path: e.path, title: e.title, portions: 0, by: [] }));
    r.portions += e.portions;
    const mine = r.by.find((b) => b.person === e.person);
    if (mine) mine.portions += e.portions;
    else r.by.push({ person: e.person, portions: e.portions });
  }
  return [...out.values()];
}
