import { describe, expect, it } from 'vitest';
import { resolveFood, suggestFoods } from './foods';
import type { Food } from './types';

const food = (name: string, synonyms: string[] = [], aisle: string | null = null): Food => ({
  name,
  aisle,
  synonyms,
  nevo: null,
  custom: null,
  negligible: false,
  piece_g: null,
  density: null,
});

const FOODS = [
  food('onion', ['ui', 'uien'], 'groente'),
  food('red onion', ['rode ui']),
  food('milk', ['melk', 'halfvolle melk'], 'zuivel'),
  food('tomato', ['tomaat', 'tomaten']),
  food('sun-dried tomatoes', ['zongedroogde tomaten']),
];

describe('foods', () => {
  it('resolves names, synonyms and plurals like the server', () => {
    expect(resolveFood(FOODS, ' Melk ')?.name).toBe('milk');
    expect(resolveFood(FOODS, 'onions')?.name).toBe('onion');
    expect(resolveFood(FOODS, 'Rode  Ui')?.name).toBe('red onion');
    expect(resolveFood(FOODS, 'toilet paper')).toBeUndefined();
    expect(resolveFood(FOODS, '')).toBeUndefined();
  });

  it('suggests exact and prefix matches first', () => {
    expect(suggestFoods(FOODS, 'ui').map((s) => [s.food.name, s.matched])).toEqual([
      ['onion', 'ui'],
      ['red onion', 'rode ui'],
    ]);
    expect(suggestFoods(FOODS, 'tom').map((s) => s.food.name)).toEqual(['tomato', 'sun-dried tomatoes']);
    expect(suggestFoods(FOODS, 'me')[0].food.name).toBe('milk');
    expect(suggestFoods(FOODS, '  ')).toEqual([]);
    expect(suggestFoods(FOODS, 'o', 2)).toHaveLength(2);
  });
});
