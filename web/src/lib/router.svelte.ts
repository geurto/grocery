import type { Meal } from './types';

// Hash routing (#/list/3): every URL is index.html as far as the server and
// the service worker are concerned, which keeps both simple.

export type Route =
  | { name: 'lists' }
  | { name: 'list'; id: number }
  | { name: 'new' }
  | { name: 'recipes' }
  | { name: 'recipe'; path: string; servings?: number }
  | { name: 'cook'; path: string; servings?: number }
  | { name: 'plan'; day?: string }
  | { name: 'plan-add'; day: string; meal: Meal | null }
  | { name: 'plan-list'; start: string };

const DAY = /^\d{4}-\d{2}-\d{2}$/;
const MEALS: readonly string[] = ['breakfast', 'lunch', 'dinner', 'snack'];

export function parse(hash: string): Route {
  const [path, query = ''] = hash.replace(/^#\/?/, '').split('?');
  const params = new URLSearchParams(query);
  const parts = path.split('/');
  if (parts[0] === 'list' && /^\d+$/.test(parts[1] ?? '')) {
    return { name: 'list', id: Number(parts[1]) };
  }
  if (parts[0] === 'new') return { name: 'new' };
  if (parts[0] === 'recipes') return { name: 'recipes' };
  if ((parts[0] === 'recipe' || parts[0] === 'cook') && parts.length > 1) {
    const servings = Number(params.get('servings'));
    return {
      name: parts[0],
      path: parts.slice(1).map(decodeURIComponent).join('/'),
      servings: servings > 0 ? servings : undefined,
    };
  }
  if (parts[0] === 'plan') {
    const day = parts[2] ?? parts[1];
    if (parts[1] === 'add' && DAY.test(parts[2] ?? '')) {
      const meal = params.get('meal');
      return { name: 'plan-add', day: parts[2], meal: meal && MEALS.includes(meal) ? (meal as Meal) : null };
    }
    if (parts[1] === 'list' && DAY.test(parts[2] ?? '')) return { name: 'plan-list', start: parts[2] };
    return { name: 'plan', day: DAY.test(day ?? '') ? day : undefined };
  }
  return { name: 'lists' };
}

export function href(route: Route): string {
  switch (route.name) {
    case 'lists':
      return '#/';
    case 'list':
      return `#/list/${route.id}`;
    case 'new':
      return '#/new';
    case 'recipes':
      return '#/recipes';
    case 'plan':
      return route.day ? `#/plan/${route.day}` : '#/plan';
    case 'plan-add':
      return `#/plan/add/${route.day}${route.meal ? `?meal=${route.meal}` : ''}`;
    case 'plan-list':
      return `#/plan/list/${route.start}`;
    case 'recipe':
    case 'cook': {
      const path = route.path.split('/').map(encodeURIComponent).join('/');
      return `#/${route.name}/${path}${route.servings ? `?servings=${route.servings}` : ''}`;
    }
  }
}

class Router {
  route = $state<Route>(parse(location.hash));
  /** Navigations inside the app, so "back" knows whether it can go back. */
  #depth = 0;

  constructor() {
    addEventListener('hashchange', () => {
      this.route = parse(location.hash);
      this.#depth++;
      scrollTo(0, 0);
    });
  }

  go(route: Route, replace = false) {
    const url = href(route);
    if (replace) {
      history.replaceState(null, '', url);
      this.route = parse(url);
    } else {
      location.hash = url;
    }
  }

  /** Back if we came from inside the app, else (opened from a link) to the overview. */
  back() {
    if (this.#depth > 0) {
      this.#depth -= 2; // this back, and the hashchange it causes
      history.back();
    } else {
      this.go({ name: 'lists' }, true);
    }
  }
}

export const router = new Router();
