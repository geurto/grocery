// Kitchen timers, as pure data: a timer is when it ends, not a countdown,
// so it survives a reload and a phone that pauses the app.

export interface Timer {
  id: string;
  /** "Saus · 20 min", shown on the timer. */
  label: string;
  /** The recipe it belongs to, to get back to it. */
  path: string;
  servings?: number;
  /** Epoch ms. */
  endsAt: number;
}

/** Milliseconds left; negative once it has gone off. */
export function remaining(t: Timer, now: number): number {
  return t.endsAt - now;
}

export function ringing(t: Timer, now: number): boolean {
  return t.endsAt <= now;
}

/** "4:05", "1:02:00"; counts up as "+0:12" once it has gone off. */
export function clock(ms: number): string {
  const over = ms < 0;
  const total = over ? Math.floor(-ms / 1000) : Math.ceil(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, '0');
  const body = h ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
  return over ? `+${body}` : body;
}

/** Add time; to a timer that has gone off, from now. */
export function extend(t: Timer, ms: number, now: number): Timer {
  return { ...t, endsAt: Math.max(now, t.endsAt) + ms };
}
