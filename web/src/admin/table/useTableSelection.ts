import * as React from 'react';

import type { DataTableSelection } from '../DataTable';

/**
 * The rows ticked in a `DataTable`. A row that left the list (deleted, filtered
 * out) leaves the selection too, so an action never reaches a row out of sight.
 */
export function useTableSelection<T>(
  rows: readonly T[],
  rowKey: (row: T) => string,
  isSelectable?: (row: T) => boolean,
): {
  selection: DataTableSelection<T>;
  selectedRows: T[];
  clear: () => void;
} {
  const [selected, setSelected] = React.useState<ReadonlySet<string>>(() => new Set());

  const selectedRows = rows.filter((row) => selected.has(rowKey(row)));
  const present = new Set(selectedRows.map(rowKey));

  return {
    selectedRows,
    clear: () => setSelected(new Set()),
    selection: {
      selected: present,
      onChange: setSelected,
      ...(isSelectable ? { isSelectable } : {}),
    },
  };
}
