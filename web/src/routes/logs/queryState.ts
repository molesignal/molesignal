import * as React from 'react';

import { detectStatementLanguage } from '@/shell/query/builder/model';
import type { QueryEditorMode } from '@/shell/query/ModeSwitch';
import { useQueryTabSearchParams, useQueryTabState } from '@/shell/queryTabs';

import { escapeSqlIdentifier } from './fieldQueryModel';

export const DEFAULT_LOG_LIMIT = 200;

/** The language of the statement in use: a field query, or SQL. */
export type LogQueryMode = 'fields' | 'sql';

export function defaultLogQuery(stream: string): string {
  return `SELECT * FROM "${escapeSqlIdentifier(stream)}"\nORDER BY _timestamp DESC\nLIMIT ${DEFAULT_LOG_LIMIT}`;
}

/** The field query a link asks for: its own `q`, or one built from the signal params it carries. */
export function requestedLogFieldQueryFromParams(params: URLSearchParams): string {
  const direct = params.get('q') ?? params.get('query') ?? '';
  if (direct.trim()) return direct.trim();
  const clauses: string[] = [];
  const traceId = params.get('trace_id') ?? params.get('traceId');
  const spanId = params.get('span_id') ?? params.get('spanId');
  const service = params.get('service') ?? params.get('service_name');
  const host = params.get('host') ?? params.get('hostname');
  const path = params.get('path') ?? params.get('route');
  const method = params.get('method');
  const status = params.get('status_code') ?? params.get('status');
  if (traceId) clauses.push(logParamClause('trace_id', traceId));
  if (spanId) clauses.push(logParamClause('span_id', spanId));
  if (service) clauses.push(logParamClause('service', service));
  if (host) clauses.push(logParamClause('host', host));
  if (path) clauses.push(logParamClause('path', path));
  if (method) clauses.push(logParamClause('method', method));
  if (status) clauses.push(logParamClause('status_code', status));
  return clauses.join(' AND ');
}

function logParamClause(field: string, value: string): string {
  return `${field}='${value.replace(/\\/g, '\\\\').replace(/'/g, "\\'")}'`;
}

/**
 * The query a Logs page is working on, and the one its link asks for.
 *
 * The stream, the mode and both statements are what make a Logs tab the query
 * it is, so they are kept with the tab: a reload brings them back instead of
 * starting from the link again. `requested*` is what the link says; the page
 * applies it when it changes.
 *
 * The Code editor takes either language: what is typed decides which statement
 * it is (`queryMode`). The builder only ever writes the field query.
 */
export function useLogQueryState() {
  const [searchParams] = useQueryTabSearchParams();
  const requestedStream = searchParams.get('stream') ?? '';
  const requestedFieldQuery = requestedLogFieldQueryFromParams(searchParams);
  // `?sql=<statement>` seeds the raw-SQL editor (used by Saved Views "Open" for
  // SQL-language views, which can't round-trip through the `?q=` field DSL).
  const requestedSql = searchParams.get('sql')?.trim() ?? '';
  const [queryMode, setQueryMode] = useQueryTabState<LogQueryMode>(
    'logs.queryMode',
    requestedSql ? 'sql' : 'fields',
  );
  const [stream, setStream] = useQueryTabState('logs.stream', requestedStream);
  const [query, setQuery] = useQueryTabState(
    'logs.query',
    () => requestedSql || (requestedStream ? defaultLogQuery(requestedStream) : ''),
  );
  const [fieldQuery, setFieldQuery] = useQueryTabState('logs.fieldQuery', requestedFieldQuery);
  const [editorMode, setEditorMode] = useQueryTabState<QueryEditorMode>('logs.editorMode', 'code');

  // Code holds one statement. The other language's copy is emptied as soon as
  // the typing turns out to be SQL (or stops being it), so the builder never
  // shows the first letters of a SELECT as search text.
  const setCodeStatement = React.useCallback(
    (statement: string) => {
      const sql = detectStatementLanguage(statement) === 'sql';
      setQueryMode(sql ? 'sql' : 'fields');
      setQuery(sql ? statement : '');
      setFieldQuery(sql ? '' : statement);
    },
    [setFieldQuery, setQuery, setQueryMode],
  );
  const setBuilderStatement = React.useCallback(
    (statement: string) => {
      setQueryMode('fields');
      setFieldQuery(statement);
    },
    [setFieldQuery, setQueryMode],
  );

  return {
    editorMode,
    setEditorMode,
    setCodeStatement,
    setBuilderStatement,
    requestedStream,
    requestedFieldQuery,
    requestedSql,
    queryMode,
    setQueryMode,
    stream,
    setStream,
    query,
    setQuery,
    fieldQuery,
    setFieldQuery,
  };
}

/** The hint in an empty Code editor: both languages it takes. */
export function logQueryPlaceholder(stream: string): string {
  const fields = 'trace_id = "..." / service_name contains "checkout"';
  return stream ? `${fields}\nSELECT * FROM "${escapeSqlIdentifier(stream)}" WHERE ...` : fields;
}

/** What a Logs tab is querying, in the few words a tab has room for. */
export function logQuerySummary({
  stream,
  queryMode,
  query,
  fieldQuery,
}: {
  stream: string;
  queryMode: LogQueryMode;
  query: string;
  fieldQuery: string;
}): string {
  const name = stream.trim();
  if (queryMode === 'fields') {
    return [name, fieldQuery.trim()].filter(Boolean).join(' · ');
  }
  const statement = query.trim();
  // The statement a stream starts with says nothing its name does not.
  if (!statement || (name && statement === defaultLogQuery(name))) return name;
  return statement.replace(/\s+/g, ' ');
}
