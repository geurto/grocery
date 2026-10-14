// Calendar days as "YYYY-MM-DD" strings in local time, which is what the
// server stores. Weeks start on Monday.

export function iso(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function parseDay(day: string): Date {
  const [y, m, d] = day.split('-').map(Number);
  return new Date(y, m - 1, d);
}

export function today(): string {
  return iso(new Date());
}

export function addDays(day: string, n: number): string {
  const d = parseDay(day);
  d.setDate(d.getDate() + n);
  return iso(d);
}

/** The Monday of the week `day` is in. */
export function weekStart(day: string): string {
  const d = parseDay(day);
  return addDays(day, -((d.getDay() + 6) % 7));
}

export function weekDays(start: string): string[] {
  return Array.from({ length: 7 }, (_, i) => addDays(start, i));
}

/** "Wednesday 14 October". */
export function longDay(day: string): string {
  return parseDay(day).toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'long' });
}

/** "12–18 Oct", "28 Sep – 4 Oct". */
export function weekLabel(start: string): string {
  const a = parseDay(start);
  const b = parseDay(addDays(start, 6));
  const month = (d: Date) => d.toLocaleDateString(undefined, { month: 'short' });
  return a.getMonth() === b.getMonth()
    ? `${a.getDate()}–${b.getDate()} ${month(b)}`
    : `${a.getDate()} ${month(a)} – ${b.getDate()} ${month(b)}`;
}
