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

/** A length of time in seconds; min < max for a range ("2-3 minutes"). */
export interface Seconds {
  min: number;
  max: number;
}

export type StepItem =
  | { type: 'text'; text: string }
  /** `amount` is for this step, scaled to the servings asked for. */
  | { type: 'ingredient'; name: string; amount: string | null }
  | { type: 'cookware'; name: string }
  | { type: 'timer'; name: string | null; text: string; seconds: Seconds | null }
  | { type: 'quantity'; text: string };

export type Step =
  // `items` is missing in recipes cached by an older version of the app.
  | { type: 'step'; text: string; items?: StepItem[]; seconds?: Seconds }
  | { type: 'note'; text: string };

export interface Section {
  name: string | null;
  steps: Step[];
}

export interface Display {
  info: string;
  ingredients: string[];
  /** Missing in recipes cached by an older version of the app. */
  time?: {
    /** As the recipe's metadata says: "40 min". */
    stated: string | null;
    stated_minutes: number | null;
    /** All timers added up: a lower bound. */
    timers: Seconds | null;
  };
  sections: Section[];
}

export interface Recipe extends RecipeSummary {
  display?: Display;
}

export interface Person {
  id: number;
  name: string;
}

export type Meal = 'breakfast' | 'lunch' | 'dinner' | 'snack';

export interface PlanEntry {
  id: number;
  person: number;
  /** YYYY-MM-DD */
  day: string;
  meal: Meal | null;
  path: string;
  title: string;
  portions: number;
}

export interface NutrientRow {
  code: string;
  label: string;
  unit: string;
  /** EU reference intake, same unit. */
  ri: number | null;
  indent: boolean;
  section: string | null;
}

export interface DayNutrition {
  day: string;
  /** Only nutrients some counted food has a value for. */
  nutrients: Record<string, { value: number; lower_bound: boolean }>;
  missing: { food: string; nutrients: string[] }[];
  entries: {
    id: number;
    kcal: number | null;
    kcal_lower_bound: boolean;
    coverage: number | null;
    problem: string | null;
  }[];
  own_foods: string[];
}

export interface PlanNutrition {
  citation: string;
  rows: NutrientRow[];
  days: DayNutrition[];
}

export type ServerEvent =
  | { type: 'list'; id: number }
  | { type: 'recipes' }
  | { type: 'plan'; person: number }
  | { type: 'people' }
  | { type: 'resync' };
