// The list being put together on the "New list" screen. Module state, so
// it survives a detour to look at a recipe.

export interface Pick {
  path: string;
  title: string;
  /** null for recipes that don't say how many they serve (can't scale). */
  servings: number | null;
}

export const draft = $state({ title: '', picks: [] as Pick[] });

export function resetDraft() {
  draft.title = '';
  draft.picks = [];
}
