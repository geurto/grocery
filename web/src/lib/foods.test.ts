import { describe, expect, it } from 'vitest';
import { amountText, parseTyped, resolveFood, suggestFoods } from './foods';
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

describe('parseTyped', () => {
  it('splits an amount off before or after the name', () => {
    expect(parseTyped('500 g kaas')).toEqual({ name: 'kaas', amount: 500, unit: 'g' });
    expect(parseTyped('500g  jonge kaas')).toEqual({ name: 'jonge kaas', amount: 500, unit: 'g' });
    expect(parseTyped('2 bananen')).toEqual({ name: 'bananen', amount: 2 });
    expect(parseTyped('1/2 kg uien')).toEqual({ name: 'uien', amount: 0.5, unit: 'kg' });
    expect(parseTyped('kaas 1,5 kg')).toEqual({ name: 'kaas', amount: 1.5, unit: 'kg' });
    expect(parseTyped('cashews 25g')).toEqual({ name: 'cashews', amount: 25, unit: 'g' });
    expect(parseTyped('2 blik tomaten')).toEqual({ name: 'tomaten', amount: 2, unit: 'blik' });
  });

  it('leaves names alone when there is no amount, or nothing else', () => {
    expect(parseTyped(' toilet paper ')).toEqual({ name: 'toilet paper' });
    expect(parseTyped('7up')).toEqual({ name: '7up' });
    expect(parseTyped('500')).toEqual({ name: '500' });
    expect(parseTyped('2 bos')).toEqual({ name: 'bos', amount: 2 });
    expect(parseTyped('0 g kaas')).toEqual({ name: '0 g kaas' });
  });

  it('formats an amount for showing', () => {
    expect(amountText(500, 'g')).toBe('500 g');
    expect(amountText(1 / 3)).toBe('0.33');
  });
});
