import * as React from 'react';
import { useSearchParams } from 'react-router-dom';

import {
  type DimensionKey,
  DEFAULT_DIMENSION,
  DEFAULT_METRIC,
  DEFAULT_SORT,
  type Direction,
  type DrawerMetric,
  drawerMetricOf,
  type MetricKey,
  parseDimension,
  parseDirection,
  parseMetric,
  parseSort,
  type SortKey,
} from './model';
import { type OpenView, readOpenView } from './openView';
import type { Changes } from './useAddress';

/** What the performance page shows, as its address says it. */
export interface PerformanceView {
  /** The level in front: the chart and the grades are of it, and so is the column that is lit. */
  metric: MetricKey;
  /** What the views are told apart by in the table. */
  by: DimensionKey;
  sort: SortKey;
  /** The way the column is ordered when it is not the one it reads in; none leaves it to the column. */
  direction: Direction | undefined;
  /** Words the values of the dimension are narrowed to. */
  search: string;
  /** The value of the dimension open in the drawer; none is no drawer. */
  open: string;
  /** The vital the drawer is of. */
  tab: DrawerMetric;
  /** The view open in the drawer of its load, over the one above. */
  drawer: OpenView | null;
}

/** The parameters of the address, by what they hold. */
export type ViewChange = Changes;

export function readView(params: URLSearchParams): PerformanceView {
  const metric = parseMetric(params.get('vital'));

  return {
    metric,
    by: parseDimension(params.get('by')),
    sort: parseSort(params.get('sort')),
    direction: parseDirection(params.get('dir')),
    search: params.get('q') ?? '',
    open: params.get('open') ?? '',
    tab: drawerMetricOf(params.get('tab'), metric),
    drawer: readOpenView(params),
  };
}

/**
 * The address after a change. What a page opens on is left out of it, so that the address of a page
 * that was not touched is the address it has; what is open in the drawer belongs to the dimension
 * it was chosen in, and goes when the dimension changes.
 */
export function applyChange(current: URLSearchParams, changes: ViewChange): URLSearchParams {
  const next = new URLSearchParams(current);

  for (const [name, value] of Object.entries(changes)) {
    if (value) next.set(name, value);
    else next.delete(name);
  }

  if ('by' in changes) {
    if (!('open' in changes)) next.delete('open');
    if (!('tab' in changes)) next.delete('tab');
  }

  if (next.get('vital') === DEFAULT_METRIC) next.delete('vital');
  if (next.get('by') === DEFAULT_DIMENSION) next.delete('by');
  if (next.get('sort') === DEFAULT_SORT) next.delete('sort');

  return next;
}

/**
 * The state of the page lives in its address: the level in front, what the table is of and how it
 * is ordered, and the value that is open, so a view can be sent as it stands. A change of what is
 * shown replaces the entry; opening a value adds one, so Back closes it.
 */
export function usePerformanceView() {
  const [params, setParams] = useSearchParams();
  const view = React.useMemo(() => readView(params), [params]);

  const set = React.useCallback(
    (changes: ViewChange, options: { push?: boolean } = {}) =>
      setParams((current) => applyChange(current, changes), { replace: !options.push }),
    [setParams],
  );

  return { view, set };
}
