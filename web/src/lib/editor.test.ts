import { describe, expect, it } from 'vitest';
import {
  cookwareSnippet,
  ingredientSnippet,
  insertAt,
  parseAmount,
  parseTags,
  parseUnits,
  timerSnippet,
} from './editor';

describe('editor helpers', () => {
  it('parses tags', () => {
    expect(parseTags('soep, Vegetarisch ,, soep')).toEqual(['soep', 'Vegetarisch']);
    expect(parseTags('')).toEqual([]);
  });

  it('parses amounts typed on a phone', () => {
    expect(parseAmount('2')).toBe(2);
    expect(parseAmount(' 2,5 ')).toBe(2.5);
    expect(parseAmount('1/2')).toBe(0.5);
    expect(parseAmount('½')).toBe(0.5);
    expect(parseAmount('')).toBeUndefined();
    expect(parseAmount('een handje')).toBeNull();
    expect(parseAmount('1/0')).toBeNull();
    expect(parseAmount('-2')).toBeNull();
  });

  it('parses unit weights', () => {
    expect(parseUnits('clove=5, blik=400')).toEqual({ clove: 5, blik: 400 });
    expect(parseUnits('')).toEqual({});
    expect(parseUnits('clove')).toBeNull();
    expect(parseUnits('blik=0')).toBeNull();
  });

  it('builds Cooklang snippets', () => {
    expect(ingredientSnippet({ name: 'ui', amount: '2' })).toBe('@ui{2}');
    expect(ingredientSnippet({ name: 'rode peper', amount: '1,5', unit: 'el' })).toBe('@rode peper{1.5%el}');
    expect(ingredientSnippet({ name: 'zout' })).toBe('@zout{}');
    expect(ingredientSnippet({ name: 'basilicum', amount: 'een handje' })).toBe('@basilicum{een handje}');
    expect(ingredientSnippet({ name: 'ui', amount: '2', note: 'fijngesnipperd', optional: true })).toBe(
      '@?ui{2}(fijngesnipperd)',
    );
    // Characters that mean something in Cooklang can't end up in a name.
    expect(ingredientSnippet({ name: 'a{b}@c', amount: '1' })).toBe('@a b c{1}');
    expect(timerSnippet({ amount: '10', unit: 'minutes' })).toBe('~{10%minutes}');
    expect(timerSnippet({ amount: 'x', unit: 'minutes' })).toBe('');
    expect(cookwareSnippet(' grote pan ')).toBe('#grote pan{}');
  });

  it('inserts at the cursor with sensible spacing', () => {
    expect(insertAt('Kook ', 5, 5, '@rijst{}')).toEqual({ text: 'Kook @rijst{}', cursor: 13 });
    expect(insertAt('Kook', 4, 4, '@rijst{}').text).toBe('Kook @rijst{}');
    expect(insertAt('Kook  en bak', 5, 5, '@rijst{}').text).toBe('Kook @rijst{} en bak');
    expect(insertAt('Kook rijst.', 5, 10, '@rijst{}').text).toBe('Kook @rijst{}.');
    expect(insertAt('', 0, 0, '@x{}')).toEqual({ text: '@x{}', cursor: 4 });
  });
});
