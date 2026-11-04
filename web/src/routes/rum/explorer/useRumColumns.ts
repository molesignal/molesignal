import * as React from 'react';

import type { RumField } from '@/api/rum';
import { useQueryTabState } from '@/shell/queryTabs';

import { defaultTableColumns, type RumTab, tableColumns } from './model';

/**
 * The columns of the table of each type, as the reader arranged them. What was chosen is kept with
 * the query tab; a type nobody arranged opens with its own columns.
 */
export function useRumColumns(tab: RumTab, fields: readonly RumField[]) {
  const [arranged, setArranged] = useQueryTabState<Record<string, string[]>>('rum.columns', {});
  const chosen = arranged[tab] ?? null;

  const columns = React.useMemo(() => tableColumns(tab, fields, chosen), [chosen, fields, tab]);
  const names = React.useMemo(() => columns.map((column) => column.name), [columns]);

  const toggle = React.useCallback(
    (name: string) => {
      const current = chosen ?? tableColumns(tab, fields, null).map((column) => column.name);
      const next = current.includes(name) ? current.filter((column) => column !== name) : [...current, name];

      setArranged((all) => ({ ...all, [tab]: next }));
    },
    [chosen, fields, setArranged, tab],
  );

  const reset = React.useCallback(() => {
    setArranged((all) => {
      const { [tab]: _dropped, ...rest } = all;
      return rest;
    });
  }, [setArranged, tab]);

  return {
    columns,
    names,
    isVisible: (name: string) => names.includes(name),
    toggle,
    reset,
    arranged: chosen !== null,
    defaults: defaultTableColumns(tab),
  };
}
