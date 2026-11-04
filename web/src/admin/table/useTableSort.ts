import * as React from 'react';

import type { DataTableColumn } from '../DataTable';
import { sortRows, type TableSort } from './model';

/** The order of a list that is paged: it has to sort all its rows before it takes a page of them. */
export function useTableSort<T>(
  rows: readonly T[],
  columns: readonly DataTableColumn<T>[],
  initial: TableSort | null = null,
) {
  const [sort, setSort] = React.useState<TableSort | null>(initial);
  const column = sort ? columns.find((candidate) => candidate.key === sort.key) : undefined;
  const value = column?.sortValue ?? column?.value;

  // `columns` is drawn anew each time, and its `value` with it; the key says which one it is.
  const sortedRows = React.useMemo(
    () => (sort && value ? sortRows(rows, value, sort.direction) : [...rows]),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [rows, sort],
  );

  return { sort, setSort, sortedRows };
}
