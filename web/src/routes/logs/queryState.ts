import * as React from 'react';

import type { StreamField } from '@/api/streams';
import { detectStatementLanguage } from '@/shell/query/builder/model';
import type { QueryEditorMode } from '@/shell/query/ModeSwitch';
import { useQueryTabSearchParams, useQueryTabState } from '@/shell/queryTabs';

import {
  type LogLinkGap,
  type LogLinkRequest,
  logLinkKey,
  logLinkStatement,
  type LogMeaning,
} from './conditions/model';
import { escapeSqlIdentifier } from './fieldQueryModel';

export const DEFAULT_LOG_LIMIT = 200;

/** The language of the statement in use: a field query, or SQL. */
export type LogQueryMode = 'fields' | 'sql';

export function defaultLogQuery(stream: string): string {
  return `SELECT * FROM "${escapeSqlIdentifier(stream)}"\nORDER BY _timestamp DESC\nLIMIT ${DEFAULT_LOG_LIMIT}`;
}

/** The field query a link carries as it is written, in its own `q`. */
export function requestedLogFieldQueryFromParams(params: URLSearchParams): string {
  return (params.get('q') ?? params.get('query') ?? '').trim();
}

/** The params a link may say each meaning in; the first with a value counts. */
const LINK_PARAMS: ReadonlyArray<readonly [LogMeaning, readonly string[]]> = [
  ['trace_id', ['traceId']],
  ['span_id', ['spanId']],
  ['service', ['service', 'serviceName']],
  ['host', ['host', 'hostname']],
  ['pod', ['pod', 'podName']],
  ['route', ['route', 'path']],
  ['operation', ['operation', 'operationName']],
  ['method', ['method']],
  ['status', ['status', 'statusCode']],
  ['environment', ['environment']],
];

/**
 * What a link asks of the logs by meaning, as a cross-signal link or a
 * dashboard's data link says it: `service`, `host`, `trace_id`. Which field
 * holds each depends on the stream, so the field query is written once the
 * stream is known. `null` when the link carries a field query of its own, or
 * asks for nothing of the kind.
 */
export function requestedLogLinkFromParams(params: URLSearchParams): LogLinkRequest | null {
  if (requestedLogFieldQueryFromParams(params)) return null;

  const request: LogLinkRequest = {};
  for (const [meaning, names] of LINK_PARAMS) {
    const value = names.map((name) => params.get(name)?.trim()).find(Boolean);
    if (value) request[meaning] = value;
  }

  return Object.keys(request).length > 0 ? request : null;
}

/** A query that ran, as its address says it. */
export interface LogQueryRun {
  stream: string;
  mode: LogQueryMode;
  statement: string;
}

/**
 * The address of a query that ran: its stream, and its statement in `q` or in
 * `sql` by its language. What a link asked for by meaning (`service`,
 * `trace_id`) is written into the statement by then, so those params go.
 */
export function writeLogQueryParams(current: URLSearchParams, run: LogQueryRun): URLSearchParams {
  const next = new URLSearchParams(current);
  for (const key of ['q', 'query', 'sql', ...LINK_PARAMS.flatMap(([, names]) => names)]) next.delete(key);

  const stream = run.stream.trim();
  const statement = run.statement.trim();
  if (stream) next.set('stream', stream);
  else next.delete('stream');
  if (statement) next.set(run.mode === 'sql' ? 'sql' : 'q', statement);

  return next;
}

/** What the address says of the query: the page applies it only when it says something else. */
interface LogQueryAddress {
  stream: string;
  fieldQuery: string;
  sql: string;
}

const NO_GAPS: readonly LogLinkGap[] = [];

/**
 * The query a Logs page is working on, and the one its link asks for.
 *
 * The stream, the mode and both statements are what make a Logs tab the query
 * it is, so they are kept with the tab: a reload brings them back instead of
 * starting from the link again. `requested*` is what the link says; the page
 * applies it when it changes.
 *
 * A link that asks by meaning (`requestedLink`) is written into the field
 * query here, in the names of the stream's own fields, once `fieldsOf` knows
 * them; until then it is pending.
 *
 * The Code editor takes either language: what is typed decides which statement
 * it is (`queryMode`). The builder only ever writes the field query.
 *
 * A query that runs is written to the address (`publishQuery`), so a copy of
 * the address is the query on screen. `addressTaken` is what the page has taken
 * from its address or written there itself: what the address still says is not
 * applied again, only what a link or the history changes there.
 */
export function useLogQueryState(fieldsOf: (stream: string) => readonly StreamField[] | undefined) {
  const [searchParams, setSearchParams] = useQueryTabSearchParams();
  const requestedStream = searchParams.get('stream') ?? '';
  const requestedFieldQuery = requestedLogFieldQueryFromParams(searchParams);
  const requestedLink = React.useMemo(() => requestedLogLinkFromParams(searchParams), [searchParams]);
  const requestedLinkKey = requestedLink ? logLinkKey(requestedLink) : '';
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
  // The link the field query was written from: a reload does not write it again over what
  // the reader has made of it since. Another link's conditions wait for their stream's fields.
  const [writtenLink, setWrittenLink] = useQueryTabState('logs.writtenLink', '');
  const [linkGaps, setLinkGaps] = React.useState<{ stream: string; gaps: LogLinkGap[] } | null>(null);
  const addressTaken = React.useRef<LogQueryAddress>({
    stream: requestedStream,
    fieldQuery: requestedFieldQuery,
    sql: requestedSql,
  });
  const linkPending = requestedLinkKey !== '' && writtenLink !== requestedLinkKey;
  const linkFields = linkPending && stream ? fieldsOf(stream) : undefined;

  React.useEffect(() => {
    if (!linkPending || !requestedLink || !linkFields) return;

    const { statement, gaps } = logLinkStatement(requestedLink, linkFields);
    // One update: the page runs the query it sees the link written into.
    setQueryMode('fields');
    setFieldQuery(statement);
    setWrittenLink(requestedLinkKey);
    setLinkGaps({ stream, gaps });
  }, [linkFields, linkPending, requestedLink, requestedLinkKey, setFieldQuery, setQueryMode, setWrittenLink, stream]);

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
  const publishQuery = React.useCallback(
    (run: LogQueryRun) => {
      const statement = run.statement.trim();
      addressTaken.current = {
        stream: run.stream.trim(),
        fieldQuery: run.mode === 'fields' ? statement : '',
        sql: run.mode === 'sql' ? statement : '',
      };
      setSearchParams((current) => writeLogQueryParams(current, run), { replace: true });
    },
    [setSearchParams],
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
    requestedLink,
    linkPending,
    // What the link asked for that the stream in use keeps no field for.
    linkGaps: linkGaps?.stream === stream ? linkGaps.gaps : NO_GAPS,
    requestedSql,
    addressTaken,
    publishQuery,
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
  const fields = 'trace_id = "..." / service.name contains "checkout"';
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
