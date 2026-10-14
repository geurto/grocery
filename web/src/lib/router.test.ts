import { describe, expect, it, vi } from 'vitest';

// The router module also creates a Router, which needs a browser; only the
// pure functions are tested here.
vi.stubGlobal('location', { hash: '' });
vi.stubGlobal('addEventListener', () => {});
const { href, parse } = await import('./router.svelte');

describe('routes', () => {
  it('round-trip', () => {
    for (const route of [
      { name: 'lists' as const },
      { name: 'list' as const, id: 3 },
      { name: 'new' as const },
      { name: 'recipes' as const },
      { name: 'recipe' as const, path: 'soups/Tomato soup & bread.cook', servings: 3 },
      { name: 'recipe' as const, path: 'Shakshuka.cook', servings: undefined },
      { name: 'cook' as const, path: 'Diner/Chana masala.cook', servings: 2 },
      { name: 'plan' as const, day: undefined },
      { name: 'plan' as const, day: '2026-10-14' },
      { name: 'plan-add' as const, day: '2026-10-14', meal: 'dinner' as const },
      { name: 'plan-add' as const, day: '2026-10-14', meal: null },
      { name: 'plan-list' as const, start: '2026-10-12' },
    ]) {
      expect(parse(href(route))).toEqual(route);
    }
  });

  it('unknown hashes go to the overview', () => {
    expect(parse('')).toEqual({ name: 'lists' });
    expect(parse('#/list/abc')).toEqual({ name: 'lists' });
  });
});
