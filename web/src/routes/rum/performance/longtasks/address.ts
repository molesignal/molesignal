import { type OpenView, readOpenView } from '../openView';
import { type Changes, useAddressState, written } from '../useAddress';
import { DEFAULT_GROUP, DEFAULT_SORT, type Direction, type GroupKey, parseDirection, parseGroup, parseSort, type SortKey } from './model';

/** What the long tasks view shows, as its address says it. */
export interface LongTasksState {
  /** What the table is of: the groups of long tasks it lists. */
  group: GroupKey;
  /** Words of the address of the script the long tasks are narrowed to. */
  search: string;
  sort: SortKey;
  /** The way the column is ordered when it is not the one it reads in; none leaves it to the column. */
  direction: Direction | undefined;
  /** The group open in the panel, as its values written in a list; none is no panel. */
  open: string;
  /** The view of a long task open in the drawer, with its waterfall. */
  drawer: OpenView | null;
}

export function readState(params: URLSearchParams): LongTasksState {
  return {
    group: parseGroup(params.get('by')),
    search: params.get('q') ?? '',
    sort: parseSort(params.get('sort')),
    direction: parseDirection(params.get('dir')),
    open: params.get('open') ?? '',
    drawer: readOpenView(params),
  };
}

/**
 * The address after a change. What a page opens on is left out of it, and the group open in the panel
 * belongs to the way the table was told apart, so it goes when that changes.
 */
export function applyChange(current: URLSearchParams, changes: Changes): URLSearchParams {
  const next = written(current, changes);

  if ('by' in changes && !('open' in changes)) next.delete('open');
  if (next.get('by') === DEFAULT_GROUP) next.delete('by');
  if (next.get('sort') === DEFAULT_SORT) next.delete('sort');

  return next;
}

export const useLongTasksState = () => useAddressState(readState, applyChange);
