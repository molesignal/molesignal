import {
  DEFAULT_GROUP,
  DEFAULT_SORT,
  type Direction,
  type GroupKey,
  type Kind,
  parseDirection,
  parseGroup,
  parseKind,
  parseSort,
  type SortKey,
} from './model';
import { type Changes, useAddressState, written } from '../performance/useAddress';

/** What the frustration tab shows, as its address says it. */
export interface FrustrationState {
  /** What the table is of: the groups of frustrated actions it lists. */
  group: GroupKey;
  /** The sign the table is narrowed to; none is every sign. */
  kind: Kind | '';
  /** Words of an element or a page the table is narrowed to. */
  search: string;
  sort: SortKey;
  /** The way the column is ordered when it is not the one it reads in; none leaves it to the column. */
  direction: Direction | undefined;
  /** The group open in the panel, as its values written in a list; none is no panel. */
  open: string;
}

export function readState(params: URLSearchParams): FrustrationState {
  return {
    group: parseGroup(params.get('by')),
    kind: parseKind(params.get('kind')),
    search: params.get('q') ?? '',
    sort: parseSort(params.get('sort')),
    direction: parseDirection(params.get('dir')),
    open: params.get('open') ?? '',
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

export const useFrustrationState = () => useAddressState(readState, applyChange);
