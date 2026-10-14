// Hash routing (#/list/3): every URL is index.html as far as the server and
// the service worker are concerned, which keeps both simple.

export type Route =
  | { name: 'lists' }
  | { name: 'list'; id: number }
  | { name: 'new' }
  | { name: 'recipe'; path: string; servings?: number };

export function parse(hash: string): Route {
  const [path, query = ''] = hash.replace(/^#\/?/, '').split('?');
  const params = new URLSearchParams(query);
  const parts = path.split('/');
  if (parts[0] === 'list' && /^\d+$/.test(parts[1] ?? '')) {
    return { name: 'list', id: Number(parts[1]) };
  }
  if (parts[0] === 'new') return { name: 'new' };
  if (parts[0] === 'recipe' && parts.length > 1) {
    const servings = Number(params.get('servings'));
    return {
      name: 'recipe',
      path: parts.slice(1).map(decodeURIComponent).join('/'),
      servings: servings > 0 ? servings : undefined,
    };
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
    case 'recipe': {
      const path = route.path.split('/').map(encodeURIComponent).join('/');
      return `#/recipe/${path}${route.servings ? `?servings=${route.servings}` : ''}`;
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
