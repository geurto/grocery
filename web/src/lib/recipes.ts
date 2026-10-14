// Recipe helpers shared by the recipe screens.

import type { RecipeSummary, Seconds, Step, StepItem } from './types';

/** Recipes matching a search on title, tag or ingredient. */
export function searchRecipes(recipes: readonly RecipeSummary[], query: string): RecipeSummary[] {
  const q = query.trim().toLowerCase();
  if (!q) return [...recipes];
  return recipes.filter(
    (r) =>
      r.title.toLowerCase().includes(q) ||
      r.tags.some((t) => t.toLowerCase().includes(q)) ||
      r.ingredients.some((i) => i.toLowerCase().includes(q)),
  );
}

/** By folder under Recipes/ ("Diner/Chana masala.cook" → "Diner"), in index order. */
export function byFolder(recipes: readonly RecipeSummary[]): { folder: string | null; recipes: RecipeSummary[] }[] {
  const groups = new Map<string | null, RecipeSummary[]>();
  for (const r of recipes) {
    const slash = r.path.lastIndexOf('/');
    const folder = slash > 0 ? r.path.slice(0, slash) : null;
    let group = groups.get(folder);
    if (!group) groups.set(folder, (group = []));
    group.push(r);
  }
  // Top-level recipes first.
  return [...groups]
    .sort(([a], [b]) => (a === null ? -1 : b === null ? 1 : 0))
    .map(([folder, recipes]) => ({ folder, recipes }));
}

/** "45 s", "1 min 30 s", "10 min", "1 h 5 min". Seconds only show under 10 minutes. */
export function duration(seconds: number): string {
  const s = Math.round(seconds);
  if (s < 60) return `${s} s`;
  if (s < 600 && s % 60) return `${Math.floor(s / 60)} min ${s % 60} s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return m % 60 ? `${h} h ${m % 60} min` : `${h} h`;
}

/** "10 min", or "2–3 min" for a range. */
export function durationRange(s: Seconds): string {
  if (s.min === s.max) return duration(s.min);
  const [a, b] = [duration(s.min), duration(s.max)];
  // "2 min–3 min" → "2–3 min" when the units match.
  const unit = (x: string) => x.replace(/^[\d.]+ /, '');
  return unit(a) === unit(b) && /^\d+ \D+$/.test(a) ? `${a.split(' ')[0]}–${b}` : `${a}–${b}`;
}

/**
 * What a timer is for, to label it: its name if it has one, else the
 * ingredient just before it in the step ("bak komijnzaad ~{30%seconds}").
 */
export function timerSubject(items: readonly StepItem[], index: number): string | null {
  const timer = items[index];
  if (timer?.type === 'timer' && timer.name) return timer.name;
  for (let i = index - 1; i >= 0; i--) {
    const item = items[i];
    if (item.type === 'ingredient') return item.name;
  }
  return null;
}

/** A step's items; recipes cached by an older app version only have text. */
export function stepItems(step: Extract<Step, { type: 'step' }>): StepItem[] {
  return step.items ?? [{ type: 'text', text: step.text }];
}
