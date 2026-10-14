import { describe, expect, it } from 'vitest';
import { allSteps, current, ingredientName, prune, timersLeft, toggle, type Progress } from './cooking';
import type { Display } from './types';

const display: Display = {
  info: '',
  ingredients: [],
  sections: [
    {
      name: null,
      steps: [
        { type: 'step', text: 'Chop', items: [] },
        { type: 'note', text: 'Careful' },
        { type: 'step', text: 'Fry', items: [], seconds: { min: 120, max: 180 } },
      ],
    },
    { name: 'Sauce', steps: [{ type: 'step', text: 'Simmer', items: [], seconds: { min: 600, max: 600 } }] },
  ],
};

describe('cooking', () => {
  it('numbers steps across sections, without notes', () => {
    expect(allSteps(display).map((s) => [s.n, s.section, s.text])).toEqual([
      [0, null, 'Chop'],
      [1, null, 'Fry'],
      [2, 'Sauce', 'Simmer'],
    ]);
  });

  it('finds the current step and the timers left', () => {
    const steps = allSteps(display);
    expect(current(steps, new Set())?.text).toBe('Chop');
    expect(current(steps, new Set(['Chop']))?.text).toBe('Fry');
    expect(current(steps, new Set(['Chop', 'Fry', 'Simmer']))).toBeUndefined();
    expect(timersLeft(steps, new Set())).toEqual({ min: 720, max: 780 });
    expect(timersLeft(steps, new Set(['Fry']))).toEqual({ min: 600, max: 600 });
    expect(timersLeft(steps, new Set(['Fry', 'Simmer']))).toBeNull();
  });

  it('reads ingredient names from lines', () => {
    expect(ingredientName('ui — 2 (fijngesnipperd)')).toBe('ui');
    expect(ingredientName('zout')).toBe('zout');
    expect(ingredientName('feta (optional)')).toBe('feta');
  });

  it('toggles and prunes', () => {
    expect(toggle(['a'], 'b')).toEqual(['a', 'b']);
    expect(toggle(['a', 'b'], 'a')).toEqual(['b']);
    const p = (updated: number): Progress => ({ path: 'x', started: 0, updated, steps: [], prep: [] });
    const day = 24 * 3600_000;
    expect(Object.keys(prune({ old: p(0), fresh: p(3 * day) }, 3 * day))).toEqual(['fresh']);
  });
});
