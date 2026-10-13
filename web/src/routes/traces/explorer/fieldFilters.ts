import * as React from 'react';

import type { QueryEditorMode } from '@/shell/query/ModeSwitch';

import {
  appendTraceSqlFieldFilter,
  insertTraceClause,
  type TraceFieldDef,
  type TraceQueryMode,
} from '../fieldQueryModel';
import {
  appendTraceSqlValueFilter,
  hasTraceValueFilter,
  insertTraceValueClause,
  removeTraceValueFilter,
  type TraceValueOperator,
} from '../valueFilter';

interface TraceFieldFilterInputs {
  editorMode: QueryEditorMode;
  queryMode: TraceQueryMode;
  queryDraft: string;
  sqlDraft: string;
  /** The stream a SQL statement reads; without one there is no statement to write. */
  stream: string;
  setBuilderField: (field: string | null) => void;
  setQueryMode: (mode: TraceQueryMode) => void;
  setQueryDraft: React.Dispatch<React.SetStateAction<string>>;
  setSqlDraft: React.Dispatch<React.SetStateAction<string>>;
  /** A field was put in the query for a value to be typed: the editor has to be in view. */
  onDraftStarted: () => void;
  /** A whole filter was added: the statement is run as it now stands. */
  onRun: (mode: TraceQueryMode, statement: string) => void;
}

/**
 * What the field list does to the query.
 *
 * A field's own "+" starts a comparison on it and leaves the value to be
 * typed, so it only touches the draft. A value's filter is whole, so it is
 * written and run at once: the results, and the values beside them, answer
 * the click. The same button takes it out again.
 */
export function useTraceFieldFilters({
  editorMode,
  queryMode,
  queryDraft,
  sqlDraft,
  stream,
  setBuilderField,
  setQueryMode,
  setQueryDraft,
  setSqlDraft,
  onDraftStarted,
  onRun,
}: TraceFieldFilterInputs) {
  // The builder only writes the field query, whatever Code last held.
  const filterMode: TraceQueryMode = queryMode === 'sql' && editorMode === 'code' ? 'sql' : 'fields';

  const insertField = React.useCallback((field: TraceFieldDef) => {
    if (editorMode === 'builder') {
      setBuilderField(field.name);
    } else if (filterMode === 'sql') {
      if (!stream) return;
      setSqlDraft((current) => appendTraceSqlFieldFilter(current, field, stream));
    } else {
      setQueryDraft((current) => insertTraceClause(current, field));
    }
    onDraftStarted();
  }, [editorMode, filterMode, onDraftStarted, setBuilderField, setQueryDraft, setSqlDraft, stream]);

  // The statement a value's filter is written into, and looked for in.
  const statement = filterMode === 'sql' ? sqlDraft : queryDraft;

  const isValueFiltered = React.useCallback(
    (field: TraceFieldDef, value: string, op: TraceValueOperator) =>
      hasTraceValueFilter(filterMode, statement, field, value, op, stream),
    [filterMode, statement, stream],
  );

  const filterValue = React.useCallback((field: TraceFieldDef, value: string, op: TraceValueOperator) => {
    if (filterMode === 'sql' && !stream) return;

    // A filter that is there is taken out by the button that put it in.
    const next = hasTraceValueFilter(filterMode, statement, field, value, op, stream)
      ? removeTraceValueFilter(filterMode, statement, field, value, op, stream)
      : filterMode === 'sql'
        ? appendTraceSqlValueFilter(statement, field, value, op, stream)
        : insertTraceValueClause(statement, field, value, op);

    if (filterMode === 'sql') {
      setSqlDraft(next);
    } else {
      setQueryMode('fields');
      setQueryDraft(next);
    }
    onRun(filterMode, next);
  }, [filterMode, onRun, setQueryDraft, setQueryMode, setSqlDraft, statement, stream]);

  return { filterMode, insertField, filterValue, isValueFiltered };
}
