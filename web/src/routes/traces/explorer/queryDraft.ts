import * as React from 'react';

import { detectStatementLanguage } from '@/shell/query/builder/model';
import type { QueryEditorMode } from '@/shell/query/ModeSwitch';
import { useQueryTabState } from '@/shell/queryTabs';

import type { TraceQueryMode } from '../fieldQueryModel';
import type { TraceUrlQueryState } from '../urlState';

/**
 * The query being written on the trace explorer, before it is run.
 *
 * `editorMode` is how it is written: as a statement (Code) or put together from
 * filters (Builder). `queryMode` is the language of the statement in use. The
 * Code editor takes either language and what is typed decides which it is; the
 * builder only ever writes the field query.
 */
export function useTraceQueryDraft(initial: TraceUrlQueryState) {
  const [editorMode, setEditorMode] = useQueryTabState<QueryEditorMode>(
    'traces.editorMode',
    'code',
  );
  // A field picked in the field list while the builder is open starts a filter there.
  const [builderField, setBuilderField] = React.useState<string | null>(null);
  const [queryMode, setQueryMode] = React.useState<TraceQueryMode>(initial.mode);
  const [queryDraft, setQueryDraft] = React.useState(initial.fields);
  const [sqlDraft, setSqlDraft] = React.useState(initial.sql);

  // Code holds one statement. The other language's copy is emptied as soon as
  // the typing turns out to be SQL (or stops being it), so the builder never
  // shows the first letters of a SELECT as search text.
  const changeCodeDraft = React.useCallback((value: string) => {
    const sql = detectStatementLanguage(value) === 'sql';
    setQueryMode(sql ? 'sql' : 'fields');
    setSqlDraft(sql ? value : '');
    setQueryDraft(sql ? '' : value);
  }, []);
  const changeBuilderDraft = React.useCallback((value: string) => {
    setQueryMode('fields');
    setQueryDraft(value);
  }, []);

  return {
    editorMode,
    setEditorMode,
    builderField,
    setBuilderField,
    queryMode,
    setQueryMode,
    queryDraft,
    setQueryDraft,
    sqlDraft,
    setSqlDraft,
    changeCodeDraft,
    changeBuilderDraft,
  };
}
