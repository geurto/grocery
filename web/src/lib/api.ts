// Thin wrapper around the server's JSON API.

/** An item typed in, optionally with an amount (`unit` missing = pieces). */
export interface NewItem {
  name: string;
  amount?: number;
  unit?: string;
}

import type {
  List,
  ListStatus,
  ListSummary,
  Food,
  FoodNutrition,
  Meal,
  NevoHit,
  NewFood,
  Person,
  PlanEntry,
  PlanNutrition,
  Recipe,
  RecipeCheck,
  RecipeFile,
  RecipeSource,
  RecipeSummary,
} from './types';

/** The server answered with an error status. */
export class HttpError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

/** The server couldn't be reached (offline, or it's down). */
export class OfflineError extends Error {}

async function request<T>(method: string, url: string, body?: unknown): Promise<T> {
  let resp: Response;
  try {
    resp = await fetch(url, {
      method,
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
      cache: 'no-store',
    });
  } catch {
    throw new OfflineError('No connection');
  }
  const isJson = resp.headers.get('content-type')?.startsWith('application/json');
  if (!resp.ok) {
    // A proxy in front of a stopped server answers 502/504: that's offline too.
    if (resp.status === 502 || resp.status === 503 || resp.status === 504) {
      throw new OfflineError(`Server unavailable (${resp.status})`);
    }
    const message = isJson ? ((await resp.json()).error ?? resp.statusText) : resp.statusText;
    throw new HttpError(resp.status, message);
  }
  if (resp.status === 204) return undefined as T;
  return (isJson ? resp.json() : resp.text()) as Promise<T>;
}

const enc = (path: string) => path.split('/').map(encodeURIComponent).join('/');

export const api = {
  lists: () => request<ListSummary[]>('GET', '/api/lists'),
  list: (id: number) => request<List>('GET', `/api/lists/${id}`),
  listText: (id: number, format: 'md' | 'txt') =>
    request<string>('GET', `/api/lists/${id}?format=${format}`),
  createList: (recipes: { path: string; servings?: number }[], title?: string, items: NewItem[] = []) =>
    request<List>('POST', '/api/lists', { recipes, title, items }),
  addRecipes: (list: number, recipes: { path: string; servings?: number }[]) =>
    request<List>('POST', `/api/lists/${list}/recipes`, { recipes }),
  updateList: (id: number, change: { title?: string; status?: ListStatus }) =>
    request<void>('PATCH', `/api/lists/${id}`, change),
  deleteList: (id: number) => request<void>('DELETE', `/api/lists/${id}`),
  addItem: (list: number, item: NewItem) =>
    request<{ id: number }>('POST', `/api/lists/${list}/items`, item),
  setChecked: (list: number, item: number, checked: boolean) =>
    request<void>('PATCH', `/api/lists/${list}/items/${item}`, { checked }),
  removeItem: (list: number, item: number) =>
    request<void>('DELETE', `/api/lists/${list}/items/${item}`),

  recipes: () => request<RecipeSummary[]>('GET', '/api/recipes'),
  recipe: (path: string, servings?: number) =>
    request<Recipe>(
      'GET',
      `/api/recipes/${enc(path)}${servings ? `?servings=${servings}` : ''}`,
    ),
  recipeText: (path: string, format: 'md' | 'txt', servings?: number) =>
    request<string>(
      'GET',
      `/api/recipes/${enc(path)}?format=${format}${servings ? `&servings=${servings}` : ''}`,
    ),
  nutrition: (path: string) => request<string>('GET', `/api/nutrition/${enc(path)}`),

  checkRecipe: (source: RecipeSource) =>
    request<RecipeCheck>('POST', '/api/recipe-check', { source }),
  recipeFolders: () => request<string[]>('GET', '/api/recipe-folders'),
  recipeFile: (path: string) => request<RecipeFile>('GET', `/api/recipe-files/${enc(path)}`),
  createRecipe: (folder: string, source: RecipeSource) =>
    request<{ path: string }>('POST', '/api/recipe-files', { folder, source }),
  saveRecipe: (path: string, source: RecipeSource, version: string) =>
    request<{ path: string }>('PUT', `/api/recipe-files/${enc(path)}`, { source, version }),
  foods: () => request<Food[]>('GET', '/api/foods'),
  aisles: () => request<string[]>('GET', '/api/aisles'),
  foodNutrition: (name: string) =>
    request<FoodNutrition>('GET', `/api/foods/${encodeURIComponent(name)}/nutrition`),
  addFood: (food: NewFood) => request<{ name: string }>('POST', '/api/foods', food),
  nevoSearch: (q: string) =>
    request<NevoHit[]>('GET', `/api/nevo/search?q=${encodeURIComponent(q)}&limit=12`),

  people: () => request<Person[]>('GET', '/api/people'),
  addPerson: (name: string) => request<Person>('POST', '/api/people', { name }),
  renamePerson: (id: number, name: string) => request<void>('PATCH', `/api/people/${id}`, { name }),
  deletePerson: (id: number) => request<void>('DELETE', `/api/people/${id}`),
  plan: (person: number, from: string, to: string) =>
    request<PlanEntry[]>('GET', `/api/plan?person=${person}&from=${from}&to=${to}`),
  planNutrition: (person: number, from: string, to: string) =>
    request<PlanNutrition>('GET', `/api/plan/nutrition?person=${person}&from=${from}&to=${to}`),
  addEntry: (
    entry: { person: number; day: string; meal: Meal | null } & (
      | { recipe: string; portions?: number }
      | { food: string; amount: number; unit?: string }
    ),
  ) => request<PlanEntry>('POST', '/api/plan', entry),
  updateEntry: (
    id: number,
    change: { day?: string; meal?: Meal | null; portions?: number; amount?: number; unit?: string },
  ) =>
    request<PlanEntry>('PATCH', `/api/plan/${id}`, change),
  deleteEntry: (id: number) => request<void>('DELETE', `/api/plan/${id}`),
};
