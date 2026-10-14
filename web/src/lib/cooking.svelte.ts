// Cooking progress per recipe, remembered on this device only.

import { prune, toggle, type Progress } from './cooking';
import { load, save } from './storage';

class Cooking {
  all = $state<Record<string, Progress>>(prune(load('cooking', {}), Date.now()));

  get(path: string): Progress | undefined {
    return this.all[path];
  }

  #update(path: string, change: (p: Progress) => Partial<Progress>, servings?: number) {
    const now = Date.now();
    const p = this.all[path] ?? { path, servings, started: now, updated: now, steps: [], prep: [] };
    this.all[path] = { ...p, ...change(p), updated: now };
    save('cooking', this.all);
  }

  start(path: string, servings?: number) {
    this.#update(path, () => ({ servings }), servings);
  }

  toggleStep(path: string, text: string) {
    this.#update(path, (p) => ({ steps: toggle(p.steps, text) }));
  }

  togglePrep(path: string, name: string) {
    this.#update(path, (p) => ({ prep: toggle(p.prep, name) }));
  }

  clear(path: string) {
    delete this.all[path];
    save('cooking', this.all);
  }
}

export const cooking = new Cooking();
