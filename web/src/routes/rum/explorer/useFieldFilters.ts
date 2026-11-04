import * as React from 'react';

import type { RumField } from '@/api/rum';
import type { QueryEditorMode } from '@/shell/query/ModeSwitch';

import { insertRumClause } from './fieldQuery';
import {
  hasRumValueFilter,
  insertRumValueClause,
  removeRumValueFilter,
  type RumValueOperator,
} from './valueFilter';

interface RumFieldFilterInputs {
  editorMode: QueryEditorMode;
  queryDraft: string;
  setBuilderField: (field: string | null) => void;
  setQueryDraft: React.Dispatch<React.SetStateAction<string>>;
  /** A field was put in the query for a value to be typed: the editor has to be in view. */
  onDraftStarted: () => void;
  /** A whole filter was added: the statement is run as it now stands. */
  onRun: (statement: string) => void;
}

/**
 * What the field list does to the query.
 *
 * A field's own "+" starts a comparison on it and leaves the value to be typed, so it only
 * touches the draft. A value's filter is whole, so it is written and run at once: the results,
 * and the values beside them, answer the click. The same button takes it out again.
 */
export function useRumFieldFilters({
  editorMode,
  queryDraft,
  setBuilderField,
  setQueryDraft,
  onDraftStarted,
  onRun,
}: RumFieldFilterInputs) {
  const insertField = React.useCallback(
    (field: RumField) => {
      if (editorMode === 'builder') setBuilderField(field.name);
      else setQueryDraft((current) => insertRumClause(current, field));

      onDraftStarted();
    },
    [editorMode, onDraftStarted, setBuilderField, setQueryDraft],
  );

  const isValueFiltered = React.useCallback(
    (field: RumField, value: string, op: RumValueOperator) => hasRumValueFilter(queryDraft, field, value, op),
    [queryDraft],
  );

  const filterValue = React.useCallback(
    (field: RumField, value: string, op: RumValueOperator) => {
      // A filter that is there is taken out by the button that put it in.
      const next = hasRumValueFilter(queryDraft, field, value, op)
        ? removeRumValueFilter(queryDraft, field, value, op)
        : insertRumValueClause(queryDraft, field, value, op);

      setQueryDraft(next);
      onRun(next);
    },
    [onRun, queryDraft, setQueryDraft],
  );

  // Several at once are one statement: the labels of a series are a filter on each of its groups.
  const filterValues = React.useCallback(
    (entries: ReadonlyArray<{ field: RumField; value: string; op: RumValueOperator }>) => {
      const next = entries.reduce(
        (statement, entry) => insertRumValueClause(statement, entry.field, entry.value, entry.op),
        queryDraft,
      );

      if (next === queryDraft) return;

      setQueryDraft(next);
      onRun(next);
    },
    [onRun, queryDraft, setQueryDraft],
  );

  return { insertField, filterValue, filterValues, isValueFiltered };
}
