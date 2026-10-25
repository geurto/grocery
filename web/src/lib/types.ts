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
  /**
   * Who needs how much ("Curry 6", by hand 8), in the line's unit. `recipe`
   * null: added by hand. Missing or empty for older lines and items added
   * offline.
   */
  parts?: { recipe: string | null; quantity: string | null }[];
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
  /** Empty for a single food. */
  path: string;
  /** For a food: its name as typed. */
  title: string;
  portions: number;
  /** A single food from the food list, with how much ("25 g", "1"). */
  food?: { qty: number; unit: string | null; text: string } | null;
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

/** A recipe as the editor shows it (crates/grocery-core/src/author.rs). */
export interface RecipeSource {
  title: string;
  servings: number | null;
  /** As written: "40 min". */
  time: string | null;
  tags: string[];
  /** Other front matter (author, source, …) as YAML. */
  extra: string;
  /** The method, in Cooklang. */
  body: string;
}

export interface RecipeFile {
  path: string;
  source: RecipeSource;
  /** Sent back when saving: the save is refused if the file changed. */
  version: string;
  note: string | null;
}

export interface CheckedIngredient {
  name: string;
  amount: string | null;
  optional: boolean;
  /** `unknown`: not in aliases.yaml yet. */
  status: 'known' | 'unknown';
  food: string | null;
  nutrition: 'nevo' | 'custom' | 'negligible' | 'none' | null;
  /** Whether this amount converts to grams. */
  weight: 'ok' | 'missing' | null;
}

export interface RecipeCheck {
  error: string | null;
  warnings: string[];
  ingredients: CheckedIngredient[];
  steps: number;
  timers_seconds: number | null;
}

export interface Food {
  name: string;
  aisle: string | null;
  synonyms: string[];
  nevo: number | null;
  custom: string | null;
  negligible: boolean;
  piece_g: number | null;
  density: number | null;
}

/** Nutrition of one food per 100 g or ml (`/api/foods/{name}/nutrition`). */
export interface FoodNutrition {
  food: string;
  source: 'nevo' | 'custom' | 'negligible' | 'none';
  nevo: { code: number; name: string } | null;
  own_source: string | null;
  per: '100g' | '100ml' | null;
  piece_g: number | null;
  /** `value` null = unknown, not zero. */
  nutrients: { code: string; label: string; unit: string; value: number | null; indent: boolean; section: string | null }[];
  citation: string | null;
}

export interface NevoHit {
  code: number;
  name_nl: string;
  name_en: string;
  group: string;
  kcal: number | null;
}

/** Label values for a food NEVO lacks (crates/grocery-core/src/custom.rs). */
export interface OwnValues {
  per?: '100g' | '100ml';
  source?: string;
  energy_kcal?: number;
  protein?: number;
  fat?: number;
  saturated_fat?: number;
  carbohydrate?: number;
  sugars?: number;
  fibre?: number;
  salt?: number;
}

export interface NewFood {
  name: string;
  aisle?: string;
  synonyms?: string[];
  nevo?: number;
  own?: OwnValues;
  negligible?: boolean;
  piece_g?: number;
  density?: number;
  units?: Record<string, number>;
}

export type ServerEvent =
  | { type: 'list'; id: number }
  | { type: 'recipes' }
  | { type: 'plan'; person: number }
  | { type: 'people' }
  | { type: 'resync' };
