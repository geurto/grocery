// Meal plans: people, each one's week, and its nutrition. Cached on the
// device for reading offline; changes need the server.

import { api, HttpError, OfflineError } from './api';
import { addDays, weekStart } from './dates';
import { load, save } from './storage';
import { store } from './store.svelte';
import type { Meal, Person, PlanEntry, PlanNutrition } from './types';

/** Weeks kept on the device. */
const KEEP_WEEKS = 12;

export const MEALS: { meal: Meal; label: string }[] = [
  { meal: 'breakfast', label: 'Breakfast' },
  { meal: 'lunch', label: 'Lunch' },
  { meal: 'dinner', label: 'Dinner' },
  { meal: 'snack', label: 'Snacks' },
];

const key = (person: number, start: string) => `${person}|${start}`;

class Planner {
  people = $state<Person[]>(load('people', []));
  /** Whose plan this device shows. */
  me = $state<number | null>(load('me', null));
  weeks = $state<Record<string, PlanEntry[]>>(load('planWeeks', {}));
  nutrition = $state<Record<string, PlanNutrition>>(load('planNutrition', {}));
  /** Why there's no nutrition for a week ("needs NEVO", …). */
  nutritionNote = $state<Record<string, string>>({});

  #loading = new Map<string, Promise<void>>();

  constructor() {
    store.on((event) => {
      if (event.type === 'people' || event.type === 'resync') void this.refreshPeople();
      if (event.type === 'plan' || event.type === 'resync') {
        for (const k of Object.keys(this.weeks)) {
          const [person, start] = k.split('|');
          if (event.type === 'resync' || Number(person) === event.person) {
            void this.refreshWeek(Number(person), start);
          }
        }
      }
    });
  }

  get person(): Person | undefined {
    return this.people.find((p) => p.id === this.me) ?? this.people[0];
  }

  select(id: number) {
    this.me = id;
    save('me', id);
  }

  entries(person: number, start: string): PlanEntry[] | undefined {
    return this.weeks[key(person, start)];
  }

  weekNutrition(person: number, start: string): PlanNutrition | undefined {
    return this.nutrition[key(person, start)];
  }

  async refreshPeople() {
    try {
      this.people = await api.people();
      save('people', this.people);
    } catch (e) {
      if (!(e instanceof OfflineError)) store.notice = message(e);
    }
  }

  /** Fetch a week's plan and nutrition. Concurrent calls share one request. */
  refreshWeek(person: number, start: string): Promise<void> {
    const k = key(person, start);
    const running = this.#loading.get(k);
    if (running) return running;
    const end = addDays(start, 6);
    const run = (async () => {
      try {
        const entries = await api.plan(person, start, end);
        this.weeks[k] = entries;
      } catch (e) {
        if (!(e instanceof OfflineError)) store.notice = message(e);
        return;
      }
      try {
        this.nutrition[k] = await api.planNutrition(person, start, end);
        delete this.nutritionNote[k];
      } catch (e) {
        this.nutritionNote[k] =
          e instanceof HttpError && e.status === 409
            ? 'No NEVO data on the server yet (run grocery import-nevo).'
            : e instanceof OfflineError
              ? 'Nutrition needs a connection.'
              : message(e);
      }
      this.#prune();
      save('planWeeks', this.weeks);
      save('planNutrition', this.nutrition);
    })().finally(() => this.#loading.delete(k));
    this.#loading.set(k, run);
    return run;
  }

  #prune() {
    const keys = Object.keys(this.weeks);
    for (const old of keys.slice(0, Math.max(0, keys.length - KEEP_WEEKS))) {
      delete this.weeks[old];
      delete this.nutrition[old];
    }
  }

  // ------------------------------------------------------------ changes
  // These throw on failure, with a message fit to show.

  async addPerson(name: string) {
    const person = await guard(api.addPerson(name));
    this.people = [...this.people, person];
    save('people', this.people);
    this.select(person.id);
  }

  async renamePerson(id: number, name: string) {
    await guard(api.renamePerson(id, name));
    await this.refreshPeople();
  }

  async deletePerson(id: number) {
    await guard(api.deletePerson(id));
    this.people = this.people.filter((p) => p.id !== id);
    save('people', this.people);
    if (this.me === id) this.me = null;
  }

  async add(person: number, day: string, meal: Meal | null, recipe: string, portions = 1) {
    await guard(api.addEntry({ person, day, meal, recipe, portions }));
    await this.refreshWeek(person, weekStart(day));
  }

  async addFood(person: number, day: string, meal: Meal | null, food: string, amount: number, unit?: string) {
    await guard(api.addEntry({ person, day, meal, food, amount, unit: unit || undefined }));
    await this.refreshWeek(person, weekStart(day));
  }

  async setAmount(entry: PlanEntry, amount: number, unit?: string) {
    await guard(api.updateEntry(entry.id, { amount, unit: unit || undefined }));
    await this.refreshWeek(entry.person, weekStart(entry.day));
  }

  async setPortions(entry: PlanEntry, portions: number) {
    const k = key(entry.person, weekStart(entry.day));
    // Show it right away; the refresh brings the nutrition.
    this.weeks[k] = (this.weeks[k] ?? []).map((e) => (e.id === entry.id ? { ...e, portions } : e));
    try {
      await guard(api.updateEntry(entry.id, { portions }));
    } finally {
      await this.refreshWeek(entry.person, weekStart(entry.day));
    }
  }

  async remove(entry: PlanEntry) {
    await guard(api.deleteEntry(entry.id));
    await this.refreshWeek(entry.person, weekStart(entry.day));
  }
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Turn "offline" into words for the person holding the phone. */
async function guard<T>(p: Promise<T>): Promise<T> {
  try {
    return await p;
  } catch (e) {
    if (e instanceof OfflineError) throw new Error('Planning needs a connection to the server.');
    throw e;
  }
}

export const planner = new Planner();
