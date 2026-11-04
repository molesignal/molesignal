import type { SetURLSearchParams } from 'react-router-dom';

import { type CatalogGrouping, parseGrouping } from './groups';

/** The catalog's own filters as its address says them, so a view of it can be shared. */
export interface CatalogParams {
  team: string;
  namespace: string;
  /** A tag's key, or `key=value`. */
  tag: string;
  grouping: CatalogGrouping;
  /** Only the reader's favorites. */
  favorites: boolean;
  /** Archived services listed too. */
  archived: boolean;
}

type CatalogParamKey = 'team' | 'namespace' | 'tag' | 'group' | 'favorites' | 'archived';

export function readCatalogParams(params: URLSearchParams): CatalogParams {
  return {
    team: params.get('team') ?? '',
    namespace: params.get('namespace') ?? '',
    tag: params.get('tag') ?? '',
    grouping: parseGrouping(params.get('group')),
    favorites: params.get('favorites') === '1',
    archived: params.get('archived') === '1',
  };
}

/** Writes one filter into the address, in place: a filter is not a step back to take. */
export function catalogParamWriter(setSearchParams: SetURLSearchParams) {
  return (key: CatalogParamKey, value: string | boolean) =>
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        const written = value === true ? '1' : value === false ? '' : value;
        if (written) next.set(key, written);
        else next.delete(key);
        return next;
      },
      { replace: true },
    );
}
