// localStorage that never throws: private windows, full or blocked storage
// just mean nothing is remembered.

const PREFIX = 'grocery:v1:';

export function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(PREFIX + key);
    return raw === null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}

export function save(key: string, value: unknown) {
  try {
    localStorage.setItem(PREFIX + key, JSON.stringify(value));
  } catch {
    // The app still works, it just won't remember.
  }
}

export function remove(key: string) {
  try {
    localStorage.removeItem(PREFIX + key);
  } catch {
    // ignore
  }
}
