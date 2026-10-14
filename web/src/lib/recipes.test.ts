import { describe, expect, it } from 'vitest';
import { byFolder, duration, durationRange, searchRecipes, timerSubject } from './recipes';
import type { RecipeSummary } from './types';

const r = (path: string, extra: Partial<RecipeSummary> = {}): RecipeSummary => ({
  path,
  title: path.replace(/^.*\//, '').replace('.cook', ''),
  servings: 4,
  tags: [],
  description: null,
  ingredients: [],
  ...extra,
});

describe('recipes', () => {
  it('formats durations', () => {
    expect(duration(30)).toBe('30 s');
    expect(duration(60)).toBe('1 min');
    expect(duration(90)).toBe('1 min 30 s');
    expect(duration(600)).toBe('10 min');
    expect(duration(3900)).toBe('1 h 5 min');
    expect(duration(7200)).toBe('2 h');
    expect(durationRange({ min: 120, max: 180 })).toBe('2–3 min');
    expect(durationRange({ min: 30, max: 120 })).toBe('30 s–2 min');
    expect(durationRange({ min: 90, max: 120 })).toBe('1 min 30 s–2 min');
    expect(durationRange({ min: 600, max: 600 })).toBe('10 min');
  });

  it('labels timers by name or the ingredient before them', () => {
    const t = (name: string | null) => ({ type: 'timer' as const, name, text: '', seconds: null });
    const items = [
      { type: 'text' as const, text: 'Bak ' },
      { type: 'ingredient' as const, name: 'komijnzaad', amount: '2 tl' },
      t(null),
      { type: 'text' as const, text: ', dan ' },
      t(null),
      t('rust'),
    ];
    expect(timerSubject(items, 2)).toBe('komijnzaad');
    expect(timerSubject(items, 4)).toBe('komijnzaad');
    expect(timerSubject(items, 5)).toBe('rust');
    expect(timerSubject([t(null)], 0)).toBeNull();
  });

  it('groups by folder, top level first', () => {
    const groups = byFolder([r('Diner/Curry.cook'), r('Soup.cook'), r('Ontbijt/Kwark.cook'), r('Diner/Paella.cook')]);
    expect(groups.map((g) => [g.folder, g.recipes.map((x) => x.title)])).toEqual([
      [null, ['Soup']],
      ['Diner', ['Curry', 'Paella']],
      ['Ontbijt', ['Kwark']],
    ]);
  });

  it('searches title, tags and ingredients', () => {
    const all = [r('A.cook', { tags: ['indiaas'] }), r('B.cook', { ingredients: ['tofu'] })];
    expect(searchRecipes(all, 'TOFU').map((x) => x.title)).toEqual(['B']);
    expect(searchRecipes(all, 'indi').map((x) => x.title)).toEqual(['A']);
    expect(searchRecipes(all, ' ')).toHaveLength(2);
  });
});
