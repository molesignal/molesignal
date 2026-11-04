import * as React from 'react';

import { useUiPreference } from '@/shell/uiPreference/useUiPreference';

import {
  DEFAULT_TRACE_COLUMN_IDS,
  isRootColumnCandidate,
  traceColumnKey,
  traceTableColumns,
} from './columns';
import type { TraceFieldDef } from '../../fieldQueryModel';

/** The most columns the table shows: what a table can still be read with, and what is kept at once. */
export const MAX_TRACE_COLUMNS = 12;

export type TraceColumnDropPosition = 'before' | 'after';

function parseColumns(raw: unknown): { fields: string[] } | null {
  if (!raw || typeof raw !== 'object' || !Array.isArray((raw as { fields?: unknown }).fields)) return null;
  const fields = (raw as { fields: unknown[] }).fields.filter((field): field is string => typeof field === 'string' && field.length > 0);
  return { fields: [...new Set(fields)] };
}

/** An arrangement with one column moved before or after another. */
export function moveTraceColumn(ids: readonly string[], source: string, target: string, position: TraceColumnDropPosition): string[] {
  if (source === target || !ids.includes(source) || !ids.includes(target)) return [...ids];

  const moved = ids.filter((id) => id !== source);
  moved.splice(moved.indexOf(target) + (position === 'after' ? 1 : 0), 0, source);
  return moved;
}

/**
 * An arrangement with a column shown again: one the trace carries goes back
 * to where the table opens with it, before the first of its later columns
 * still shown; a field of the root goes last.
 */
export function showTraceColumn(ids: readonly string[], id: string): string[] {
  const at = DEFAULT_TRACE_COLUMN_IDS.indexOf(id);
  const later = at < 0 ? undefined : DEFAULT_TRACE_COLUMN_IDS.slice(at + 1).find((other) => ids.includes(other));
  if (!later) return [...ids, id];

  const next = [...ids];
  next.splice(next.indexOf(later), 0, id);
  return next;
}

/**
 * The columns of the trace table as the reader arranged them, as the column
 * settings and the field list change them: which of the columns a trace
 * carries are shown and in what order, and the fields of each trace's root
 * span put beside them. Kept per trace stream as a preference of the
 * signed-in user; until one is kept the table opens with every column a trace
 * carries, as it always has.
 */
export function useTraceColumns(stream: string, fields: readonly TraceFieldDef[]) {
  const kept = useUiPreference('traces.columns', stream || null, parseColumns);
  const candidates = React.useMemo(() => new Set(fields.filter(isRootColumnCandidate).map((field) => field.name)), [fields]);

  const columns = React.useMemo(
    () => traceTableColumns(kept.value?.fields ?? DEFAULT_TRACE_COLUMN_IDS, candidates).slice(0, MAX_TRACE_COLUMNS),
    [candidates, kept.value],
  );
  const shown = React.useMemo(() => columns.map((column) => column.id), [columns]);
  const rootFields = React.useMemo(() => columns.filter((column) => column.key === null).map((column) => column.id), [columns]);
  const full = shown.length >= MAX_TRACE_COLUMNS;

  const available = React.useMemo(
    () => ({
      columns: DEFAULT_TRACE_COLUMN_IDS.filter((id) => !shown.includes(id)),
      fields: [...candidates].filter((name) => !shown.includes(name)).sort((a, b) => a.localeCompare(b)),
    }),
    [candidates, shown],
  );

  const toggle = React.useCallback(
    (id: string) => {
      // The last column stays: a table of no columns has nothing to read.
      if (shown.includes(id)) {
        if (shown.length > 1) kept.save({ fields: shown.filter((other) => other !== id) });
        return;
      }
      if (full || (!traceColumnKey(id) && !candidates.has(id))) return;
      kept.save({ fields: showTraceColumn(shown, id) });
    },
    [candidates, full, kept, shown],
  );

  const reorder = React.useCallback(
    (source: string, target: string, position: TraceColumnDropPosition) => kept.save({ fields: moveTraceColumn(shown, source, target, position) }),
    [kept, shown],
  );

  return { columns, rootFields, available, full, arranged: kept.value !== null, toggle, reorder, reset: kept.clear };
}
