// Recipes being picked to add to a list. Module state, so the picks survive
// a detour to look at a recipe.

export interface Pick {
  path: string;
  title: string;
  /** null for recipes that don't say how many they serve (can't scale). */
  servings: number | null;
}

export const draft = $state({ list: null as number | null, picks: [] as Pick[] });

/** Start picking for `list`, unless that's already going on. */
export function draftFor(list: number) {
  if (draft.list !== list) {
    draft.list = list;
    draft.picks = [];
  }
}

export function resetDraft() {
  draft.list = null;
  draft.picks = [];
}
