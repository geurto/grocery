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
      { name: 'recipe' as const, path: 'soups/Tomato soup & bread.cook', servings: 3 },
      { name: 'recipe' as const, path: 'Shakshuka.cook', servings: undefined },
    ]) {
      expect(parse(href(route))).toEqual(route);
    }
  });

  it('unknown hashes go to the overview', () => {
    expect(parse('')).toEqual({ name: 'lists' });
    expect(parse('#/list/abc')).toEqual({ name: 'lists' });
  });
});
