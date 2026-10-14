// Service worker: makes the app itself load without a connection.
//
// The data (lists, recipes, unsent ticks) is cached by the app in
// localStorage; this only caches the files that make up the app. /api
// requests pass straight through.
//
// - index.html: network first (with a timeout, as a shop's "one bar" is
//   worse than none), cached copy when that fails. A new build therefore
//   arrives on the next load with a connection.
// - /assets/*: content-hashed by Vite, so cache first, forever. Copies no
//   longer referenced by index.html are cleaned up when it changes.
// - icons and the manifest: cached copy, refreshed in the background.

const CACHE = 'grocery-app-v1';
const SHELL = ['/', '/manifest.webmanifest', '/icon.svg', '/icon-192.png', '/icon-512.png'];
const NETWORK_TIMEOUT_MS = 4000;

const assetsIn = (html) => [...html.matchAll(/(?:src|href)="(\/assets\/[^"]+)"/g)].map((m) => m[1]);

async function cacheShell() {
  const cache = await caches.open(CACHE);
  const resp = await fetch('/', { cache: 'no-cache' });
  if (!resp.ok) throw new Error(`index.html: ${resp.status}`);
  const html = await resp.clone().text();
  await cache.put('/', resp);
  await cache.addAll([...SHELL.slice(1), ...assetsIn(html)]);
  await prune(cache, html);
}

/** Drop cached assets the current index.html no longer uses. */
async function prune(cache, html) {
  const keep = new Set(assetsIn(html));
  for (const req of await cache.keys()) {
    const path = new URL(req.url).pathname;
    if (path.startsWith('/assets/') && !keep.has(path)) await cache.delete(req);
  }
}

self.addEventListener('install', (event) => {
  event.waitUntil(cacheShell().then(() => self.skipWaiting()));
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      for (const key of await caches.keys()) if (key !== CACHE) await caches.delete(key);
      await self.clients.claim();
    })(),
  );
});

function withTimeout(promise, ms) {
  return Promise.race([
    promise,
    new Promise((_, reject) => setTimeout(() => reject(new Error('timeout')), ms)),
  ]);
}

async function page() {
  const cache = await caches.open(CACHE);
  try {
    const resp = await withTimeout(fetch('/', { cache: 'no-cache' }), NETWORK_TIMEOUT_MS);
    if (resp.ok) {
      const html = await resp.clone().text();
      await cache.put('/', resp.clone());
      // Fetch the new build's assets now, so it also works offline later.
      cache.addAll(assetsIn(html)).then(() => prune(cache, html), () => {});
      return resp;
    }
  } catch {
    // offline or too slow: fall through to the cached copy
  }
  return (await cache.match('/')) ?? new Response('Offline, and the app isn’t cached yet.', { status: 503 });
}

async function asset(request) {
  const cache = await caches.open(CACHE);
  const hit = await cache.match(request);
  if (hit) return hit;
  const resp = await fetch(request);
  if (resp.ok) await cache.put(request, resp.clone());
  return resp;
}

async function staleWhileRevalidate(request) {
  const cache = await caches.open(CACHE);
  const hit = await cache.match(request);
  const update = fetch(request)
    .then((resp) => {
      if (resp.ok) cache.put(request, resp.clone());
      return resp;
    })
    .catch(() => undefined);
  return hit ?? (await update) ?? new Response('', { status: 504 });
}

self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);
  if (request.method !== 'GET' || url.origin !== self.location.origin) return;
  if (url.pathname.startsWith('/api/')) return;

  if (request.mode === 'navigate') {
    // Hash routing: every page is index.html.
    event.respondWith(page());
  } else if (url.pathname.startsWith('/assets/')) {
    event.respondWith(asset(request));
  } else if (SHELL.includes(url.pathname) || url.pathname === '/apple-touch-icon.png') {
    event.respondWith(staleWhileRevalidate(request));
  }
});

// A timer notification: bring the app to the front.
self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  event.waitUntil(
    (async () => {
      const windows = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
      if (windows.length) return windows[0].focus();
      return self.clients.openWindow('/');
    })(),
  );
});
