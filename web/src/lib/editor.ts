// Pure helpers for the recipe and food forms.

/** "soep, vegetarisch ,," → ["soep", "vegetarisch"], without repeats. */
export function parseTags(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of text.split(',')) {
    const t = raw.trim();
    if (t && !seen.has(t.toLowerCase())) {
      seen.add(t.toLowerCase());
      out.push(t);
    }
  }
  return out;
}

/**
 * A number typed on a phone: "2,5", " 1/2 ", "½". `undefined` for empty,
 * `null` when it's not a number (so a form can say so rather than drop it).
 */
export function parseAmount(text: string): number | null | undefined {
  const t = text.trim();
  if (!t) return undefined;
  const fractions: Record<string, number> = { '½': 0.5, '⅓': 1 / 3, '⅔': 2 / 3, '¼': 0.25, '¾': 0.75 };
  if (t in fractions) return fractions[t];
  const frac = /^(\d+(?:[.,]\d+)?)\s*\/\s*(\d+(?:[.,]\d+)?)$/.exec(t);
  if (frac) {
    const [n, d] = [Number(frac[1].replace(',', '.')), Number(frac[2].replace(',', '.'))];
    return d > 0 ? n / d : null;
  }
  if (!/^\d+(?:[.,]\d+)?$/.test(t)) return null;
  return Number(t.replace(',', '.'));
}

/** "clove=5, blik=400" → { clove: 5, blik: 400 }; `null` if unreadable. */
export function parseUnits(text: string): Record<string, number> | null {
  const out: Record<string, number> = {};
  for (const part of text.split(',').map((p) => p.trim()).filter(Boolean)) {
    const [name, value] = part.split(/[=:]/).map((x) => x.trim());
    const n = value === undefined ? null : parseAmount(value);
    if (!name || n === null || n === undefined || n <= 0) return null;
    out[name] = n;
  }
  return out;
}

function number(v: number): string {
  return String(Math.round(v * 1000) / 1000);
}

/** `@ui{2%stuks}(fijngesnipperd)`; always with braces, so spaces in names work. */
export function ingredientSnippet(o: {
  name: string;
  amount?: string;
  unit?: string;
  note?: string;
  optional?: boolean;
}): string {
  const name = o.name.trim().replace(/[{}@#~%()]/g, ' ').replace(/\s+/g, ' ');
  const amount = parseAmount(o.amount ?? '');
  const unit = (o.unit ?? '').trim().replace(/[{}%]/g, '');
  // Text like "een handje" is kept as text; numbers are normalized.
  const text = amount === undefined ? '' : amount === null ? (o.amount ?? '').trim() : number(amount);
  const quantity = text && unit ? `${text}%${unit}` : text;
  const note = (o.note ?? '').trim().replace(/[()]/g, '');
  return `@${o.optional ? '?' : ''}${name}{${quantity}}${note ? `(${note})` : ''}`;
}

export function timerSnippet(o: { amount: string; unit: 'seconds' | 'minutes' | 'hours'; name?: string }): string {
  const amount = parseAmount(o.amount);
  if (amount === undefined || amount === null) return '';
  const name = (o.name ?? '').trim().replace(/[{}%~]/g, '');
  return `~${name}{${number(amount)}%${o.unit}}`;
}

export function cookwareSnippet(name: string): string {
  const n = name.trim().replace(/[{}@#~%]/g, ' ').replace(/\s+/g, ' ');
  return n ? `#${n}{}` : '';
}

/**
 * Put `snippet` where the cursor or selection is, with a space before and
 * after where the neighbours would otherwise run into it.
 */
export function insertAt(
  text: string,
  start: number,
  end: number,
  snippet: string,
): { text: string; cursor: number } {
  const before = text.slice(0, start);
  const after = text.slice(end);
  const lead = before && !/[\s(]$/.test(before) ? ' ' : '';
  const trail = after && !/^[\s.,;:!?)]/.test(after) ? ' ' : '';
  const out = before + lead + snippet + trail + after;
  return { text: out, cursor: (before + lead + snippet).length };
}
