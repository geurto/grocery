import { describe, expect, it } from 'vitest';
import { exportTitle, inListOrder } from './export';

describe('export', () => {
  it('names the PDF after the recipes', () => {
    expect(exportTitle([])).toBe('Recipes');
    expect(exportTitle(['Shakshuka'])).toBe('Shakshuka');
    expect(exportTitle(['Shakshuka', 'Pasta arrabbiata'])).toBe('Shakshuka and Pasta arrabbiata');
    expect(exportTitle(['Shakshuka', 'Pasta', 'Dal'])).toBe('Shakshuka and 2 more');
  });

  it('keeps characters file names can’t have out of the title', () => {
    expect(exportTitle(['Curry: snel / makkelijk'])).toBe('Curry snel makkelijk');
  });

  it('keeps the listed order, not the order tapped', () => {
    const all = [{ path: 'a' }, { path: 'b' }, { path: 'c' }];
    expect(inListOrder(all, new Set(['c', 'a']))).toEqual([{ path: 'a' }, { path: 'c' }]);
  });
});
