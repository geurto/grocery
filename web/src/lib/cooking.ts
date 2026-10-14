// Cooking progress, as pure data. Steps and ingredients are remembered by
// their text, so ticks stay on the right step when the .cook file changes
// around them.

import type { Display, Seconds, Step } from './types';

export interface Progress {
  path: string;
  servings?: number;
  /** Epoch ms. */
  started: number;
  updated: number;
  /** Texts of the steps that are done. */
  steps: string[];
  /** Ingredient names set out ("mise en place"). */
  prep: string[];
}

/** Forget progress untouched for this long. */
export const KEEP_MS = 2 * 24 * 3600_000;

export type CookStep = Extract<Step, { type: 'step' }> & {
  /** Position in the whole method, from 0. */
  n: number;
  section: string | null;
};

/** All steps in order, without notes. */
export function allSteps(d: Display): CookStep[] {
  const out: CookStep[] = [];
  for (const section of d.sections) {
    for (const step of section.steps) {
      if (step.type === 'step') out.push({ ...step, n: out.length, section: section.name });
    }
  }
  return out;
}

/** The ingredient's name from a line like "ui — 2 (fijngesnipperd)". */
export function ingredientName(line: string): string {
  return line.split(' — ')[0].replace(/ \(optional\)$/, '');
}

/** The first step not done yet; `undefined` when all are done. */
export function current(steps: readonly CookStep[], done: ReadonlySet<string>): CookStep | undefined {
  return steps.find((s) => !done.has(s.text));
}

/** Timers in steps not done yet; `null` if none have one. */
export function timersLeft(steps: readonly CookStep[], done: ReadonlySet<string>): Seconds | null {
  const total: Seconds = { min: 0, max: 0 };
  let any = false;
  for (const s of steps) {
    if (done.has(s.text) || !s.seconds) continue;
    total.min += s.seconds.min;
    total.max += s.seconds.max;
    any = true;
  }
  return any ? total : null;
}

export function toggle(list: readonly string[], value: string): string[] {
  return list.includes(value) ? list.filter((v) => v !== value) : [...list, value];
}

/** Drop progress nobody touched in a while. */
export function prune(all: Record<string, Progress>, now: number): Record<string, Progress> {
  return Object.fromEntries(Object.entries(all).filter(([, p]) => now - p.updated < KEEP_MS));
}
