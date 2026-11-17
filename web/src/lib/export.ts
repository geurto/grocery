// Exporting recipes as a PDF: the app lays them out for print and the
// browser's print dialog saves it ("Save as PDF"). No PDF library, so it also
// works offline with recipes the phone has cached, and any text prints.

/**
 * The document title while printing: browsers offer it as the PDF's file
 * name. "Shakshuka", "Shakshuka and Pasta arrabbiata", "Shakshuka and 2 more".
 */
export function exportTitle(titles: readonly string[]): string {
  const clean = titles.map((t) => t.replace(/[\\/:*?"<>|]+/g, ' ').replace(/\s+/g, ' ').trim()).filter(Boolean);
  if (clean.length === 0) return 'Recipes';
  if (clean.length === 1) return clean[0];
  if (clean.length === 2) return `${clean[0]} and ${clean[1]}`;
  return `${clean[0]} and ${clean.length - 1} more`;
}

/** Keep `picked` in the order the recipes are listed (folder, then title). */
export function inListOrder<T extends { path: string }>(all: readonly T[], picked: ReadonlySet<string>): T[] {
  return all.filter((r) => picked.has(r.path));
}
