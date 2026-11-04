import * as React from 'react';

import { DataTable } from '@/admin';

import type { ProductStateProps } from './states';

function findDataTable(children: React.ReactNode): React.ReactElement<{ loading?: boolean }> | null {
  for (const child of React.Children.toArray(children)) {
    if (!React.isValidElement(child)) continue;
    if (child.type === DataTable) return child as React.ReactElement<{ loading?: boolean }>;
    if (child.type === React.Fragment) {
      const inner = findDataTable((child.props as { children?: React.ReactNode }).children);
      if (inner) return inner;
    }
  }
  return null;
}

/**
 * What a list shows for its state. A table on its way keeps its head and its
 * rows' place, the cells drawn as bars, so nothing moves when the rows arrive;
 * every other state, and a list that is no table, takes the whole place.
 */
export function listContent(
  state: ProductStateProps | null | undefined,
  children: React.ReactNode,
): { state: ProductStateProps | null | undefined; children: React.ReactNode } {
  if (state?.variant !== 'loading') return { state, children };

  const table = findDataTable(children);
  return table ? { state: null, children: React.cloneElement(table, { loading: true }) } : { state, children };
}
