/**
 * How many rows a page of an explorer (Logs, Traces) can hold. Signal data is
 * read in bulk: a screen of twenty says little about what a query matches, so
 * the sizes start where the other lists end.
 *
 * The largest is what the list endpoints give in one answer: `MAX_PAGE_SIZE`
 * and `MAX_TRACE_PAGE_SIZE` in the backend's `routes/web`. A larger size asked
 * of them is cut to that, so the two change together.
 */
export const EXPLORER_PAGE_SIZE_OPTIONS = [100, 200, 500, 1000, 2000, 5000];
export const DEFAULT_EXPLORER_PAGE_SIZE = 100;
export const MAX_EXPLORER_PAGE_SIZE = 5000;

/**
 * The sizes worth offering for rows that are all in hand (a statement's result,
 * paged here): none larger than the first that already shows every row.
 */
export function explorerPageSizesFor(rowCount: number): number[] {
  return EXPLORER_PAGE_SIZE_OPTIONS.filter((_, index, sizes) => index === 0 || sizes[index - 1]! < rowCount);
}

/** The size in use among those offered: the one chosen, or the largest offered when it is no longer among them. */
export function explorerPageSizeIn(sizes: readonly number[], chosen: number): number {
  return sizes.includes(chosen) ? chosen : sizes.at(-1) ?? DEFAULT_EXPLORER_PAGE_SIZE;
}
