import { DEFAULT_RUM_TAB, type RumTab, rumTabOf } from './model';
import { clearRumVizFields } from './viz/address';

/** The tab an address names; one that names none, or an unknown one, is the first of the list. */
export function rumTabFromParams(params: URLSearchParams): RumTab {
  return rumTabOf(params.get('type')) ?? DEFAULT_RUM_TAB;
}

/** The query an address carries: `q`, or its older spelling `query`. */
export function rumQueryFromParams(params: URLSearchParams): string {
  return (params.get('q') ?? params.get('query') ?? '').trim();
}

/**
 * The address of the query applied on the page: its statement in `q`, in place of whatever the
 * address asked for before. No statement is no query.
 */
export function writeRumQueryParams(current: URLSearchParams, statement: string): URLSearchParams {
  const next = new URLSearchParams(current);
  next.delete('query');

  const trimmed = statement.trim();
  if (trimmed) next.set('q', trimmed);
  else next.delete('q');

  return next;
}

/**
 * The address of the tab in front. The first tab is the one an address without `type` opens, so it is
 * left out. The fields the numbers were of (`m`, `by`, `sort`) were of the type that was left.
 */
export function writeRumTabParams(current: URLSearchParams, tab: RumTab): URLSearchParams {
  const next = clearRumVizFields(current);

  if (tab === DEFAULT_RUM_TAB) next.delete('type');
  else next.set('type', tab);

  return next;
}

/** A value inside single quotes of a field query. */
export function quoteRumValue(value: string): string {
  return value.replace(/\\/g, '\\\\').replace(/'/g, "\\'");
}
