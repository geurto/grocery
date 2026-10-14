// Shapes of the JSON the server sends (crates/grocery-server/src/api.rs).

export type ListStatus = 'open' | 'done';

export interface ListSummary {
  id: number;
  title: string;
  created: string;
  status: ListStatus;
  item_count: number;
  checked_count: number;
}

export interface Item {
  /** Negative while an item added offline hasn't reached the server. */
  id: number;
  name: string;
  aisle: string | null;
  /** Formatted: "200 g", "3 cloves". */
  quantity: string | null;
  notes: string[];
  optional: boolean;
  recipes: string[];
  checked: boolean;
}

export interface ListSource {
  path: string;
  title: string;
  servings: number | null;
}

export interface List extends ListSummary {
  recipes: ListSource[];
  items: Item[];
  pantry: string[];
  warnings?: string[];
  unmapped?: string[];
}

export interface RecipeSummary {
  path: string;
  title: string;
  servings: number | null;
  tags: string[];
  description: string | null;
  ingredients: string[];
  warnings?: string[];
  error?: string;
}

export interface Recipe extends RecipeSummary {
  display?: {
    info: string;
    ingredients: string[];
    sections: { name: string | null; steps: { type: 'step' | 'note'; text: string }[] }[];
  };
}

export type ServerEvent =
  | { type: 'list'; id: number }
  | { type: 'recipes' }
  | { type: 'resync' };
