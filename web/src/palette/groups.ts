import type { ItemKind, ResultItem } from './registry';

/** How many rows a kind shows before it offers the rest. */
export const GROUP_LIMIT = 5;

export interface PaletteGroup {
  kind: ItemKind;
  items: ResultItem[];
  /** How many there are behind `items`. */
  hidden: number;
}

/**
 * The results of a search by what they are. A kind stands where its best match
 * stood in the ranking, so the likeliest answer stays at the top, and shows its
 * first few unless it was asked to show them all.
 */
export function groupResults(ranked: readonly ResultItem[], expanded: ReadonlySet<ItemKind>): PaletteGroup[] {
  const byKind = new Map<ItemKind, ResultItem[]>();

  for (const item of ranked) {
    const list = byKind.get(item.kind);
    if (list) list.push(item);
    else byKind.set(item.kind, [item]);
  }

  return [...byKind].map(([kind, items]) => {
    const shown = expanded.has(kind) ? items : items.slice(0, GROUP_LIMIT);
    return { kind, items: shown, hidden: items.length - shown.length };
  });
}
