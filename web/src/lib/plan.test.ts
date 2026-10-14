import { describe, expect, it } from 'vitest';
import { plannedRecipes } from './plan';
import type { PlanEntry } from './types';

let id = 0;
const e = (person: number, day: string, path: string, portions = 1): PlanEntry => ({
  id: ++id,
  person,
  day,
  meal: 'dinner',
  path,
  title: path.replace('.cook', ''),
  portions,
});

describe('plannedRecipes', () => {
  it('sums portions per recipe over people and days', () => {
    const got = plannedRecipes(
      [
        e(1, '2026-10-14', 'Curry.cook'),
        e(2, '2026-10-14', 'Curry.cook', 1.5),
        e(1, '2026-10-15', 'Curry.cook'), // leftovers
        e(1, '2026-10-15', 'Paella.cook'),
        e(1, '2026-10-20', 'Soup.cook'), // not a chosen day
      ],
      new Set(['2026-10-14', '2026-10-15']),
    );
    expect(got.map((r) => [r.title, r.portions, r.by])).toEqual([
      [
        'Curry',
        3.5,
        [
          { person: 1, portions: 2 },
          { person: 2, portions: 1.5 },
        ],
      ],
      ['Paella', 1, [{ person: 1, portions: 1 }]],
    ]);
  });
});
