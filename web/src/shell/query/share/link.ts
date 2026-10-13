import { encodeFilters } from '@/shell/UrlHydration';
import type { GlobalFilter } from '@/stores/useFiltersStore';
import { type Anchor, resolveWindow, type TimeWindow, writeTimeParams } from '@/stores/useTimeStore';

/** What a link to a query has to carry for the page it opens to show the same thing. */
export interface QueryShare {
  /** The explorer's own address: `/logs`. */
  pathname: string;
  /** The query on screen, as the page's own search params say it. */
  own: URLSearchParams;
  window: TimeWindow;
  anchor: Anchor | null;
  /** The filters pinned across pages, which the query runs under. */
  filters: readonly GlobalFilter[];
}

/**
 * A window as the moments it covers at `now`. "The last hour" is an hour that
 * moves; fixed, it is this hour, whenever the link is opened.
 */
export function fixedWindow(window: TimeWindow, now: Date = new Date()): TimeWindow {
  if (window.mode === 'absolute') return window;

  const { from, to } = resolveWindow(window, now);

  return { mode: 'absolute', from: from.toISOString(), to: to.toISOString() };
}

/**
 * The search a link to the query carries: the page's own params, then the time
 * range and the pinned filters in the spellings the app reads them back in.
 */
export function queryShareSearch(share: QueryShare, options: { fixed: boolean; now?: Date }): string {
  const params = new URLSearchParams(share.own);
  // The keys the shell's stores own are written from the stores, whatever the page passed.
  for (const key of ['time', 'anchor', 'filters', 'stack']) params.delete(key);

  writeTimeParams(params, options.fixed ? fixedWindow(share.window, options.now) : share.window, share.anchor);
  if (share.filters.length > 0) params.set('filters', encodeFilters([...share.filters]));

  return params.toString();
}

/** The address that opens the query, whole: something to paste into a chat or a ticket. */
export function queryShareLink(
  share: QueryShare,
  options: { fixed: boolean; origin: string; now?: Date },
): string {
  const search = queryShareSearch(share, options);

  return `${options.origin}${share.pathname}${search ? `?${search}` : ''}`;
}
