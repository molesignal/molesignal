import { useQuery } from '@tanstack/react-query';
import { RefreshCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as fieldMaskingApi from '@/api/fieldMasking';
import * as queryApi from '@/api/query';
import * as streamsApi from '@/api/streams';
import type { LogListResponse } from '@/api/web';
import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import {
  type DateFormat,
  formatMicros,
  type TimeFormat,
  useTimeFormatter,
} from '@/lib/time';
import {
  DEFAULT_EXPLORER_PAGE_SIZE,
  EXPLORER_PAGE_SIZE_OPTIONS,
  explorerPageSizeIn,
  explorerPageSizesFor,
} from '@/pagination/explorerPageSize';
import { useCursorPagination } from '@/pagination/useCursorPagination';
import { TimeRangeChip } from '@/shell/chrome';
import type { CodeCompletionItem } from '@/shell/codeEditor/types';
import { CursorPagination } from '@/shell/CursorPagination';
import { FilterBuilder } from '@/shell/query/builder/FilterBuilder';
import { QueryEditorFrame } from '@/shell/query/EditorFrame';
import { ResultHistogram } from '@/shell/query/histogram/Chart';
import type { HistogramRange } from '@/shell/query/histogram/model';
import { QueryIconAction, QueryRunAction } from '@/shell/query/IconAction';
import { QueryModeSwitch } from '@/shell/query/ModeSwitch';
import { QueryRecoveryState } from '@/shell/query/RecoveryState';
import { QueryShareButton } from '@/shell/query/share/Button';
import { QueryState } from '@/shell/query/State';
import { QuerySyntaxHelp } from '@/shell/query/SyntaxHelp';
import { useSqlFunctionCompletions } from '@/shell/query/useSqlFunctionCompletions';
import { QueryWorkbench } from '@/shell/query/Workbench';
import { useFoldedWhileNarrow, useQueryTabStatus } from '@/shell/queryTabs';
import { ResultPagination } from '@/shell/ResultPagination';
import { WorkbenchTitle } from '@/shell/SurfaceWorkbench';
import { isNarrowViewport } from '@/shell/useViewportWidth';
import { useAuthStore } from '@/stores/auth';
import { useExplorerLayoutStore } from '@/stores/useExplorerLayoutStore';
import { useFiltersStore, type GlobalFilter } from '@/stores/useFiltersStore';
import { resolveWindow, useTimeStore } from '@/stores/useTimeStore';
import { formatTimeWindowLabel } from '@/time/TimePicker';
import type { QueryResult } from '@/types/query';

import { logBuilderDialect, logBuilderFields } from './builderDialect';
import { useLogColumns } from './columns/useLogColumns';
import { runLogCursorQuery } from './cursorQuery';
import { LogDetail } from './detail/Drawer';
import {
  appendLogFieldClause,
  appendLogFieldContainsClause,
  appendLogFieldValueClause,
  buildLogFieldQuerySql,
  deriveLogFields,
  escapeSqlIdentifier,
  escapeSqlString,
  type LogFieldDef,
} from './fieldQueryModel';
import { LogFieldPanel } from './fields/Panel';
import { type CountedLogQuery, useLogHistogramCounts } from './histogramCounts';
import type { InspectionTarget } from './inspection/model';
import { useLogInspection } from './inspection/useInspection';
import { LogInspectionView } from './inspection/View';
import { PATTERN_SAMPLE_STEP, useLogPatternSample, type PatternSampleQuery } from './patterns/sample';
import { LogPatternsView } from './patterns/View';
import {
  DEFAULT_LOG_LIMIT,
  defaultLogQuery,
  logQueryPlaceholder,
  logQuerySummary,
  useLogQueryState,
} from './queryState';
import { LogResultSummary } from './ResultSummary';
import { LogListResults } from './ResultViews';
import { LogStatsView } from './stats/View';
import { LogStreamSelect } from './StreamSelect';
import {
  logLevelTone,
  displayLogValue,
  recordsToCsv,
  recordsToLogText,
  topLogFieldValues,
  type LogEntry,
  type LogLevel,
} from './viewModel';
import { LogResultViewTabs, type LogResultView } from './views';

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function quotedCompletion(value: string): string {
  return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

function logQueryCompletions(
  fields: LogFieldDef[],
  rows: LogEntry[],
  maskedFields: ReadonlySet<string> | null,
): CodeCompletionItem[] {
  const fieldNames = new Set([
    'level',
    'message',
    'body',
    'service',
    'service_name',
    'trace_id',
    'span_id',
    'status',
    'status_code',
    'duration',
    'duration_ms',
    ...fields.map((field) => field.name),
  ]);
  const values = new Map<string, Set<string>>([
    ['level', new Set(['INFO', 'WARN', 'ERROR', 'DEBUG', 'TRACE'])],
    ['service', new Set(['checkout', 'api', 'web'])],
    ['service_name', new Set(['checkout', 'api', 'web'])],
  ]);
  // Until masking metadata is known, expose only the static, non-sensitive
  // fallbacks above. Observed log values are added only for confirmed
  // unmasked fields so autocomplete cannot become a masking side channel.
  if (maskedFields !== null) {
    for (const row of rows.slice(0, 200)) {
      if (!maskedFields.has('level')) values.get('level')?.add(row.level);
      for (const [key, value] of Object.entries(row.raw)) {
        if (!fieldNames.has(key) || maskedFields.has(key.toLowerCase())) continue;
        const bucket = values.get(key) ?? new Set<string>();
        values.set(key, bucket);
        if (bucket.size >= 50) continue;
        if (typeof value === 'string' && value.length > 0 && value.length <= 80) {
          bucket.add(value);
        }
        if (typeof value === 'number' || typeof value === 'boolean') {
          bucket.add(String(value));
        }
      }
    }
  }
  return [
    ...Array.from(fieldNames).sort().map((label) => ({ label, kind: 'field' as const, detail: 'log field' })),
    { label: '=', insertText: '= ', kind: 'operator', detail: 'operator' },
    { label: '!=', insertText: '!= ', kind: 'operator', detail: 'operator' },
    { label: '>=', insertText: '>= ', kind: 'operator', detail: 'operator' },
    { label: '<=', insertText: '<= ', kind: 'operator', detail: 'operator' },
    { label: '>', insertText: '> ', kind: 'operator', detail: 'operator' },
    { label: '<', insertText: '< ', kind: 'operator', detail: 'operator' },
    { label: 'contains', insertText: 'contains ', kind: 'operator', detail: 'operator' },
    { label: 'AND', insertText: 'AND ', kind: 'operator', detail: 'operator' },
    { label: 'OR', insertText: 'OR ', kind: 'operator', detail: 'operator' },
    ...Array.from(values.entries()).flatMap(([field, fieldValues]) => (
      Array.from(fieldValues).sort().map((value) => {
        const quoted = quotedCompletion(value);
        return {
          label: quoted,
          insertText: quoted,
          kind: 'value' as const,
          detail: `${field} value`,
          field,
          value,
        };
      })
    )),
  ];
}

function hasSqlFieldFilter(statement: string, field: string): boolean {
  const quotedField = `"${escapeSqlIdentifier(field)}"`;
  const escapedQuoted = escapeRegExp(quotedField);
  const escapedBare = escapeRegExp(field);
  const pattern = new RegExp(`(?:^|\\s|\\()(${escapedQuoted}|${escapedBare})\\s*(?:>=|<=|=|!=|<>|>|<|LIKE\\b|IN\\s*\\(|IS\\s+(?:NOT\\s+)?NULL)`, 'i');
  return pattern.test(statement);
}

function appendSqlFieldFilter(statement: string, stream: string, field: LogFieldDef): string {
  const base = statement.trim() || defaultLogQuery(stream);
  if (hasSqlFieldFilter(base, field.name)) return statement;
  const identifier = `"${escapeSqlIdentifier(field.name)}"`;
  const clause = field.dataType === 'json'
    ? `CAST(${identifier} AS VARCHAR) LIKE '%"key"%'`
    : field.dataType === 'bool'
      ? `${identifier} = TRUE`
      : field.dataType === 'timestamp'
        ? `CAST(${identifier} AS BIGINT) >= 0`
        : ['int64', 'float64'].includes(field.dataType)
        ? `${identifier} >= 0`
        : `${identifier} = ''`;
  const boundary = base.search(/\s+(ORDER\s+BY|LIMIT)\b/i);
  if (boundary >= 0) {
    const before = base.slice(0, boundary).trimEnd();
    const after = base.slice(boundary);
    const joiner = /\bWHERE\b/i.test(before) ? `\n  AND ${clause}` : `\nWHERE ${clause}`;
    return `${before}${joiner}${after}`;
  }
  return /\bWHERE\b/i.test(base) ? `${base}\n  AND ${clause}` : `${base}\nWHERE ${clause}`;
}

function appendSqlFieldValueFilter(
  statement: string,
  stream: string,
  field: string,
  value: unknown,
  mode: 'include' | 'exclude',
): string {
  const base = statement.trim() || defaultLogQuery(stream);
  const sqlValue = typeof value === 'number' && Number.isFinite(value)
    ? String(value)
    : typeof value === 'boolean'
      ? String(value).toUpperCase()
      : `'${escapeSqlString(displayLogValue(value))}'`;
  const clause = `"${escapeSqlIdentifier(field)}" ${mode === 'exclude' ? '!=' : '='} ${sqlValue}`;
  const boundary = base.search(/\s+(ORDER\s+BY|LIMIT)\b/i);
  const before = boundary >= 0 ? base.slice(0, boundary).trimEnd() : base;
  const after = boundary >= 0 ? base.slice(boundary) : '';
  const joiner = /\bWHERE\b/i.test(before) ? `\n  AND ${clause}` : `\nWHERE ${clause}`;
  return `${before}${joiner}${after}`;
}

/** The SQL form of asking for the records whose field has `text` somewhere in it. */
function appendSqlFieldContainsFilter(statement: string, stream: string, field: string, text: string): string {
  const base = statement.trim() || defaultLogQuery(stream);
  const clause = `"${escapeSqlIdentifier(field)}" LIKE '%${escapeSqlString(text)}%'`;
  if (base.includes(clause)) return base;

  const boundary = base.search(/\s+(ORDER\s+BY|LIMIT)\b/i);
  const before = boundary >= 0 ? base.slice(0, boundary).trimEnd() : base;
  const after = boundary >= 0 ? base.slice(boundary) : '';
  const joiner = /\bWHERE\b/i.test(before) ? `\n  AND ${clause}` : `\nWHERE ${clause}`;

  return `${before}${joiner}${after}`;
}

/**
 * Append pinned global filters (`useFiltersStore`) to the SQL about to run, as
 * `"key" = 'value'` clauses inserted before any ORDER BY / LIMIT. Filters whose
 * field the query already constrains are skipped (no conflict / duplication),
 * and an empty filter set is a no-op — so behavior is unchanged when nothing is
 * pinned.
 */
function appendGlobalFilters(statement: string, stream: string, filters: GlobalFilter[]): string {
  if (filters.length === 0) return statement;
  const base = statement.trim() || defaultLogQuery(stream);
  const clauses = filters
    .filter((f) => f.key && f.value && !hasSqlFieldFilter(base, f.key))
    .map((f) => `"${escapeSqlIdentifier(f.key)}" ${f.operator === '!=' ? '!=' : '='} '${f.value.replace(/'/g, "''")}'`);
  if (clauses.length === 0) return base;
  const boundary = base.search(/\s+(ORDER\s+BY|LIMIT)\b/i);
  const body = boundary >= 0 ? base.slice(0, boundary).trimEnd() : base;
  const tail = boundary >= 0 ? base.slice(boundary) : '';
  let hasWhere = /\bWHERE\b/i.test(body);
  let result = body;
  for (const clause of clauses) {
    result += hasWhere ? `\n  AND ${clause}` : `\nWHERE ${clause}`;
    hasWhere = true;
  }
  return `${result}${tail}`;
}

function isPresent(value: unknown): boolean {
  return value !== null && value !== undefined && value !== '';
}

function rowToRecord(row: unknown[], columns: string[]): Record<string, unknown> {
  return columns.reduce<Record<string, unknown>>((acc, column, index) => {
    acc[column] = row[index];
    return acc;
  }, {});
}

function formatTimestamp(
  value: unknown,
  tz: string,
  format: TimeFormat,
  dateFormat: DateFormat,
): string {
  if (!isPresent(value)) return '—';
  const micros = timestampToMicros(value);
  return micros == null
    ? String(value)
    : formatMicros(micros, tz, format, true, dateFormat);
}

function timestampToMicros(value: unknown): number | null {
  const numeric = typeof value === 'number'
    ? value
    : typeof value === 'string' && /^-?\d+(?:\.\d+)?$/.test(value.trim())
      ? Number(value)
      : null;
  if (numeric != null && Number.isFinite(numeric)) {
    const absolute = Math.abs(numeric);
    if (absolute > 1e17) return Math.floor(numeric / 1_000);
    if (absolute > 1e14) return Math.floor(numeric);
    if (absolute > 1e11) return Math.floor(numeric * 1_000);
    return Math.floor(numeric * 1_000_000);
  }
  if (typeof value !== 'string') return null;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed * 1_000;
}

function downloadTextFile(filename: string, value: string, type: string): void {
  const blob = new Blob([value], { type });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

/**
 * Map a `/query` row tuple to the display LogEntry shape using a column
 * index map. Missing display columns degrade only in presentation fields;
 * the raw row is kept intact for the source line and detail drawer.
 */
function rowToLog(
  row: unknown[],
  columns: string[],
  idx: Record<string, number>,
  tz: string,
  format: TimeFormat,
  dateFormat: DateFormat,
): LogEntry {
  const get = (k: string): unknown => (idx[k] !== undefined ? row[idx[k]!] : undefined);
  const level = String(get('level') ?? get('body_level') ?? 'INFO').toUpperCase() as LogLevel;
  const moment = get('_timestamp') ?? get('timestamp') ?? get('time');
  return {
    ts: formatTimestamp(moment, tz, format, dateFormat),
    micros: isPresent(moment) ? timestampToMicros(moment) : null,
    level,
    raw: rowToRecord(row, columns),
  };
}

/** The rows of an answer as the page shows them. */
function resultToLogs(result: QueryResult, tz: string, format: TimeFormat, dateFormat: DateFormat): LogEntry[] {
  const idx: Record<string, number> = {};
  result.columns.forEach((column, index) => {
    idx[column] = index;
  });

  return result.rows.map((row) => rowToLog(row, result.columns, idx, tz, format, dateFormat));
}

/** What opens a log in the drawer: a row of the list, or a log under a pattern. */
const LOG_OPENERS = '[data-log-result-row="logs"], [data-log-pattern-example]';

export function Logs() {
  const { t, i18n } = useTranslation('logs');
  const { t: tCommon } = useTranslation('common');
  const fmt = useTimeFormatter();
  const {
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
  } = useLogQueryState();
  // A field picked in the field list while the builder is open starts a filter there.
  const [builderField, setBuilderField] = React.useState<string | null>(null);
  // Which view of the results is shown: the logs, their statistics, an inspection, or their patterns.
  const [view, setView] = React.useState<LogResultView>('raw');
  // The histogram can be closed to give the rows more room; that choice is
  // remembered, so the page opens the way it was left.
  const showHistogram = useExplorerLayoutStore((state) => state.logsHistogramVisible);
  const setShowHistogram = useExplorerLayoutStore((state) => state.setLogsHistogramVisible);
  // Folded in a narrow window, and for as long as the page has a narrow share of a split one.
  const [fieldPanelCollapsed, setFieldPanelCollapsed] = useFoldedWhileNarrow(isNarrowViewport);
  // The editor starts folded into the bar, as on Traces: it opens when there is a
  // query to look at (one arrived in the link, a filter was added) or on request.
  const [queryEditorCollapsed, setQueryEditorCollapsed] = React.useState(
    !requestedFieldQuery && !requestedSql,
  );
  const [expandedField, setExpandedField] = React.useState<string | null>(null);
  const [resultPage, setResultPage] = React.useState(1);
  const [chosenResultPageSize, setResultPageSize] = React.useState(DEFAULT_EXPLORER_PAGE_SIZE);
  const [selectedRow, setSelectedRow] = React.useState<number | null>(null);
  const [queryResult, setQueryResult] = React.useState<QueryResult | undefined>();
  const [logCursorPage, setLogCursorPage] = React.useState<LogListResponse | null>(null);
  // The time range that produced `queryResult`, captured at run time: another page of
  // the list, the histogram and the patterns are all of that range, not of the live picker.
  const [executedTimeRange, setExecutedTimeRange] = React.useState<HistogramRange | null>(null);
  // The field query the rows came from, for the histogram to count everything it matches.
  // A statement the reader wrote has none: its histogram is of the rows it returned.
  const [countedQuery, setCountedQuery] = React.useState<CountedLogQuery | null>(null);
  // The statement the rows came from, when the reader wrote one: its result is what statistics draw.
  const [writtenStatement, setWrittenStatement] = React.useState<string | null>(null);
  const inspection = useLogInspection();
  const [queryError, setQueryError] = React.useState<unknown>(null);
  const [queryPending, setQueryPending] = React.useState(false);
  const initialRunStreamRef = React.useRef<string | null>(null);
  const appliedFieldQueryRef = React.useRef(requestedFieldQuery);
  const appliedSqlRef = React.useRef(requestedSql);

  const orgId = useAuthStore((s) => s.ctx?.org_id ?? '');
  const timeWindow = useTimeStore((s) => s.window);
  const setTimeWindow = useTimeStore((s) => s.setWindow);
  const globalFilters = useFiltersStore((s) => s.filters);
  const clearGlobalFilters = useFiltersStore((s) => s.clearFilters);
  const previousTimeWindowRef = React.useRef(timeWindow);
  const logCursorContextKey = React.useMemo(
    () =>
      JSON.stringify({
        orgId,
        stream,
        fieldQuery,
        globalFilters,
        timeWindow,
      }),
    [fieldQuery, globalFilters, orgId, stream, timeWindow],
  );
  const {
    pageSize: logPageSize,
    reset: resetLogCursor,
    goPrevious: goToPreviousLogPage,
    goNext: goToNextLogPage,
    setPageSize: setLogPageSize,
  } = useCursorPagination({
    contextKey: logCursorContextKey,
    defaultPageSize: DEFAULT_EXPLORER_PAGE_SIZE,
  });

  const streamsQuery = useQuery({
    queryKey: ['streams', 'logs-selector'],
    queryFn: () => streamsApi.list(500),
  });
  // Logs always queries with `stream_type: 'logs'`, so the selector must only
  // offer logs streams. Otherwise the auto-select picks `streams[0]` — which
  // after on-demand creation can be a traces/metrics stream (e.g. a `default`
  // traces stream from a test trace) — and the query 404s with
  // `stream not found: default`.
  const streams = React.useMemo(
    () => (streamsQuery.data ?? []).filter((s) => s.type === 'logs' && streamsApi.isQueryable(s)),
    [streamsQuery.data],
  );

  const setAutoStream = React.useCallback((nextStream: string, options?: { preserveFieldQuery?: boolean }) => {
    const nextQuery = defaultLogQuery(nextStream);
    setStream(nextStream);
    setQuery(nextQuery);
    if (!options?.preserveFieldQuery) setFieldQuery('');
    setSelectedRow(null);
    setResultPage(1);
    setQueryResult(undefined);
    setLogCursorPage(null);
    setQueryError(null);
    // The editor keeps whatever state the user left it in: picking a stream
    // (the first one is picked on load) is not a reason to reopen it.
    setExpandedField(null);
  }, [setFieldQuery, setQuery, setStream]);

  const executeQuery = React.useCallback(async (
    options: { cursor?: string; pageSize?: number } = {},
  ) => {
    const selectedStream = stream.trim();
    if (!selectedStream) {
      setQueryError(new Error('Select a stream before querying.'));
      return;
    }
    setQueryPending(true);
    setQueryError(null);
    try {
      const resolvedWindow = resolveWindow(timeWindow);
      const queryTimeRange = options.cursor && executedTimeRange
        ? executedTimeRange
        : {
            start: resolvedWindow.from.getTime() * 1000,
            end: resolvedWindow.to.getTime() * 1000,
          };
      const baseStatement = queryMode === 'sql'
        ? (query.trim() || defaultLogQuery(selectedStream))
        : buildLogFieldQuerySql(selectedStream, fieldQuery, options.pageSize ?? logPageSize);
      const statement = appendGlobalFilters(baseStatement, selectedStream, globalFilters);
      let result: QueryResult;
      if (queryMode === 'fields') {
        const { page, result: cursorResult } = await runLogCursorQuery({
          stream: selectedStream,
          statement: fieldQuery,
          globalFilters,
          timeRange: queryTimeRange,
          pageSize: options.pageSize ?? logPageSize,
          cursor: options.cursor,
        });
        if (!options.cursor) resetLogCursor();
        setLogCursorPage(page);
        result = cursorResult;
      } else {
        resetLogCursor();
        setLogCursorPage(null);
        result = await queryApi.runQuery({
          org_id: orgId,
          language: 'sql',
          statement,
          time_range: queryTimeRange,
          stream: { name: selectedStream, stream_type: 'logs' },
          limit: DEFAULT_LOG_LIMIT,
        });
      }
      setQueryResult(result);
      setExecutedTimeRange(queryTimeRange);
      // Another page is of the same query: what it matches has been counted.
      if (!options.cursor) {
        setCountedQuery(
          queryMode === 'fields'
            ? { orgId, stream: selectedStream, statement: fieldQuery, globalFilters, ranAt: Date.now() }
            : null,
        );
        setWrittenStatement(queryMode === 'sql' ? statement : null);
      }
      setSelectedRow(null);
      setResultPage(1);
    } catch (err) {
      setQueryError(err);
      // What the last run returned is not the answer to this one: its count, its
      // histogram and its field counts would stand beside the error as if they were.
      setQueryResult(undefined);
      setLogCursorPage(null);
      setCountedQuery(null);
      setWrittenStatement(null);
    } finally {
      setQueryPending(false);
    }
  }, [
    executedTimeRange,
    fieldQuery,
    globalFilters,
    logPageSize,
    orgId,
    query,
    queryMode,
    resetLogCursor,
    stream,
    timeWindow,
  ]);

  React.useEffect(() => {
    if (!requestedStream || requestedStream === stream) return;
    setAutoStream(requestedStream, { preserveFieldQuery: Boolean(requestedFieldQuery) });
  }, [requestedFieldQuery, requestedStream, setAutoStream, stream]);

  React.useEffect(() => {
    if (stream || streams.length === 0) return;
    // Streams are queried by name (`stream: { name }`), so the default
    // selection must be the stream *name* — using `.id` here makes the backend
    // reject with `stream not found: <id>`.
    setAutoStream(streams[0]!.name, { preserveFieldQuery: Boolean(requestedFieldQuery) });
  }, [requestedFieldQuery, setAutoStream, stream, streams]);

  React.useEffect(() => {
    if (requestedFieldQuery === appliedFieldQueryRef.current) return;
    appliedFieldQueryRef.current = requestedFieldQuery;
    setQueryMode('fields');
    setFieldQuery(requestedFieldQuery);
    setSelectedRow(null);
    setResultPage(1);
    setQueryResult(undefined);
    setLogCursorPage(null);
    setQueryError(null);
    setQueryEditorCollapsed(false);
    initialRunStreamRef.current = null;
  }, [requestedFieldQuery, setFieldQuery, setQueryMode]);

  // Apply the raw-SQL seed (`?sql=`) with the same state reset as a field query,
  // then enter SQL mode with the statement verbatim and let auto-run fire.
  React.useEffect(() => {
    if (!requestedSql || requestedSql === appliedSqlRef.current) return;
    appliedSqlRef.current = requestedSql;
    setQueryMode('sql');
    setQuery(requestedSql);
    setSelectedRow(null);
    setResultPage(1);
    setQueryResult(undefined);
    setLogCursorPage(null);
    setQueryError(null);
    setQueryEditorCollapsed(false);
    initialRunStreamRef.current = null;
  }, [requestedSql, setQuery, setQueryMode]);

  React.useEffect(() => {
    const runKey = `${orgId}:${stream}`;
    if (!orgId || !stream || (queryMode === 'sql' && !query.trim()) || initialRunStreamRef.current === runKey) return;
    initialRunStreamRef.current = runKey;
    void executeQuery();
    // Runs once for the active org + stream. Re-runs are explicit through the
    // query / refresh controls.
  }, [executeQuery, orgId, query, queryMode, stream]);

  const rows = React.useMemo<LogEntry[]>(
    () => (queryResult ? resultToLogs(queryResult, fmt.tz, fmt.format, fmt.dateFormat) : []),
    [queryResult, fmt.tz, fmt.format, fmt.dateFormat],
  );
  // A page of the list is too few logs to say what kinds of message there are: the patterns
  // read the same query further back. A statement the reader wrote has only the rows it returned.
  const patternQuery = React.useMemo<PatternSampleQuery | null>(
    () =>
      view === 'patterns' && countedQuery && executedTimeRange
        ? {
            ...countedQuery,
            timeRange: executedTimeRange,
            // A reader who pages the list by more than a step reads the patterns by as much.
            step: Math.max(PATTERN_SAMPLE_STEP, logPageSize),
          }
        : null,
    [countedQuery, executedTimeRange, logPageSize, view],
  );
  const patternSample = useLogPatternSample(rows.length > 0 ? patternQuery : null);
  // The logs in view: the page of the list, or what the patterns were read from once the first of it is here.
  // The fields beside them, the histogram and the drawer are of these.
  const sampled = patternSample.result !== undefined;
  const viewResult = patternSample.result ?? queryResult;
  const viewRows = React.useMemo(
    () =>
      patternSample.result ? resultToLogs(patternSample.result, fmt.tz, fmt.format, fmt.dateFormat) : rows,
    [fmt.dateFormat, fmt.format, fmt.tz, patternSample.result, rows],
  );
  const rawRecords = React.useMemo(() => viewRows.map((row) => row.raw), [viewRows]);
  const rowMoments = React.useMemo(() => viewRows.map((row) => row.micros), [viewRows]);

  // A log is open by its place among the logs in view: among other logs that place is another log.
  React.useEffect(() => setSelectedRow(null), [sampled]);

  const cursorResults = queryMode === 'fields';
  // What a statement returned is all in hand and paged here: no size is offered beyond the one that shows it all.
  const resultPageSizes = React.useMemo(() => explorerPageSizesFor(rows.length), [rows.length]);
  const resultPageSize = explorerPageSizeIn(resultPageSizes, chosenResultPageSize);
  const resultPageCount = cursorResults
    ? 1
    : Math.max(1, Math.ceil(rows.length / resultPageSize));
  const activeResultPage = Math.min(resultPage, resultPageCount);
  const resultPageStart = cursorResults ? 0 : (activeResultPage - 1) * resultPageSize;
  const pagedRows = React.useMemo(
    () => cursorResults ? rows : rows.slice(resultPageStart, resultPageStart + resultPageSize),
    [cursorResults, resultPageSize, resultPageStart, rows],
  );

  React.useEffect(() => {
    setResultPage((current) => Math.min(current, resultPageCount));
  }, [resultPageCount]);

  const changeResultPage = React.useCallback((nextPage: number) => {
    setResultPage(Math.min(Math.max(1, nextPage), resultPageCount));
    setSelectedRow(null);
  }, [resultPageCount]);

  const changeResultPageSize = React.useCallback((nextPageSize: number) => {
    setResultPageSize(nextPageSize);
    setResultPage(1);
    setSelectedRow(null);
  }, []);

  const changeLogPage = React.useCallback((direction: 'previous' | 'next') => {
    const token = direction === 'previous'
      ? logCursorPage?.previous_cursor
      : logCursorPage?.next_cursor;
    if (!token) return;
    if (direction === 'previous') goToPreviousLogPage(logCursorPage);
    else goToNextLogPage(logCursorPage);
    setSelectedRow(null);
    void executeQuery({ cursor: token });
  }, [executeQuery, goToNextLogPage, goToPreviousLogPage, logCursorPage]);

  const changeLogPageSize = React.useCallback((nextPageSize: number) => {
    setLogPageSize(nextPageSize);
    setSelectedRow(null);
    void executeQuery({ pageSize: nextPageSize });
  }, [executeQuery, setLogPageSize]);

  const histogramCounts = useLogHistogramCounts(countedQuery);
  const histogramPoints = React.useMemo(
    () => viewRows.map((row) => ({ micros: row.micros, error: logLevelTone(row.level) === 'error' })),
    [viewRows],
  );
  const selectHistogramRange = React.useCallback(
    ({ from, to }: { from: number; to: number }) =>
      setTimeWindow({
        mode: 'absolute',
        from: new Date(from / 1000).toISOString(),
        to: new Date(to / 1000).toISOString(),
      }),
    [setTimeWindow],
  );

  const closeLogDetail = React.useCallback(() => {
    const row = document.querySelector<HTMLElement>(`:is(${LOG_OPENERS})[aria-pressed="true"]`);

    setSelectedRow(null);
    // The keyboard goes back to the row the drawer was opened from.
    row?.focus();
  }, []);

  const selectedStream = React.useMemo(
    () => streams.find((candidate) => candidate.name === stream && candidate.stream_type === 'logs'),
    [stream, streams],
  );
  const selectedStreamFields = React.useMemo(
    () => selectedStream?.schema.fields ?? [],
    [selectedStream],
  );
  const fieldMaskingQuery = useQuery({
    queryKey: ['field-masking-effective', selectedStream?.id],
    queryFn: () => fieldMaskingApi.effectiveForStream(selectedStream?.id ?? ''),
    enabled: Boolean(selectedStream?.id),
  });
  const maskedFields = React.useMemo(
    () => fieldMaskingQuery.data
      ? new Set(
          fieldMaskingQuery.data.fields
            .filter((field) => field.masked)
            .map((field) => field.field.toLowerCase()),
        )
      : null,
    [fieldMaskingQuery.data],
  );
  const fields = React.useMemo(
    () => deriveLogFields(selectedStreamFields, viewResult),
    [selectedStreamFields, viewResult],
  );
  const builderFields = React.useMemo(() => logBuilderFields(fields), [fields]);
  const suggestBuilderValues = React.useCallback(
    (field: string) => topLogFieldValues(rawRecords, field, 8).map((item) => item.label),
    [rawRecords],
  );
  // SQL 检索函数（MATCH / MATCH_TEXT）由后端能力接口驱动；Fields 与 SQL 模式都注入。
  const sqlFunctions = useSqlFunctionCompletions();
  const completionItems = React.useMemo(
    () => [...sqlFunctions, ...logQueryCompletions(fields, rows, maskedFields)],
    [sqlFunctions, fields, maskedFields, rows],
  );

  // Which fields stand beside the time, in what order: the reader's own arrangement for this
  // stream where one is kept, what the stream opens with otherwise.
  const logColumns = useLogColumns(stream, fields);
  const visibleLogFields = logColumns.visibleFields;
  const toggleLogFieldVisibility = logColumns.toggle;
  const reorderLogField = logColumns.reorder;

  const insertLogFieldFilter = React.useCallback((field: string) => {
    if (!stream) return;
    const definition = fields.find((candidate) => candidate.name === field);
    if (!definition) return;
    setQueryEditorCollapsed(false);
    if (editorMode === 'builder') {
      setBuilderField(field);
      return;
    }
    if (queryMode === 'sql') {
      setQuery((current) => appendSqlFieldFilter(current, stream, definition));
      return;
    }
    setFieldQuery((current) => appendLogFieldClause(current, definition));
  }, [editorMode, fields, queryMode, setFieldQuery, setQuery, stream]);

  const insertLogFieldValueFilter = React.useCallback((
    field: string,
    value: unknown,
    filterMode: 'include' | 'exclude',
  ) => {
    if (!stream) return;
    const definition = fields.find((candidate) => candidate.name === field);
    // The builder only writes the field query, whatever Code last held.
    const sqlActive = queryMode === 'sql' && editorMode === 'code';
    if (!sqlActive && definition?.dataType === 'json') return;
    setQueryEditorCollapsed(false);
    if (sqlActive) {
      setQuery((current) => appendSqlFieldValueFilter(current, stream, field, value, filterMode));
      return;
    }
    setQueryMode('fields');
    setFieldQuery((current) => appendLogFieldValueClause(current, field, value, filterMode));
  }, [editorMode, fields, queryMode, setFieldQuery, setQuery, setQueryMode, stream]);

  // A pattern is asked for by the longest run of words all of its logs have.
  const insertLogPatternFilter = React.useCallback((field: string, fragment: string) => {
    if (!stream) return;

    setQueryEditorCollapsed(false);
    // The builder only writes the field query, whatever Code last held.
    if (queryMode === 'sql' && editorMode === 'code') {
      setQuery((current) => appendSqlFieldContainsFilter(current, stream, field, fragment));
      return;
    }
    setQueryMode('fields');
    setFieldQuery((current) => appendLogFieldContainsClause(current, field, fragment));
  }, [editorMode, queryMode, setFieldQuery, setQuery, setQueryMode, stream]);

  // A statement the statistics view wrote goes into the editor as the reader's own, and is run:
  // the run-once guard is lifted, so the effect below runs it with the state as it then is.
  const openStatement = React.useCallback((statement: string) => {
    setQueryMode('sql');
    setQuery(statement);
    setQueryEditorCollapsed(false);
    initialRunStreamRef.current = null;
  }, [setQuery, setQueryMode]);
  const inspectionTarget = React.useMemo<InspectionTarget | null>(() => {
    if (!executedTimeRange) return null;
    if (countedQuery) return { stream: countedQuery.stream, statement: countedQuery.statement, range: executedTimeRange };

    return writtenStatement && stream ? { stream, statement: writtenStatement, range: executedTimeRange } : null;
  }, [countedQuery, executedTimeRange, stream, writtenStatement]);

  const activeQueryText = queryMode === 'sql' ? query.trim() : fieldQuery.trim();

  const clearActiveQuery = React.useCallback(() => {
    if (queryMode === 'sql') {
      setQuery('');
    } else {
      setFieldQuery('');
    }
  }, [queryMode, setFieldQuery, setQuery]);

  const downloadVisibleCsv = React.useCallback(() => {
    if (rows.length === 0) return;
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const columns = ['_timestamp', ...visibleLogFields.filter((field) => field !== '_timestamp')];
    downloadTextFile(
      `molesignal-logs-${timestamp}.csv`,
      recordsToCsv(rawRecords, columns),
      'text/csv;charset=utf-8',
    );
  }, [rawRecords, rows.length, visibleLogFields]);

  const downloadVisibleLog = React.useCallback(() => {
    if (rows.length === 0) return;
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const columns = ['_timestamp', ...visibleLogFields.filter((field) => field !== '_timestamp')];
    downloadTextFile(
      `molesignal-logs-${timestamp}.log`,
      recordsToLogText(rawRecords, columns),
      'text/plain;charset=utf-8',
    );
  }, [rawRecords, rows.length, visibleLogFields]);

  const hasRunnableQuery = queryMode === 'fields' || query.trim().length > 0;
  const canRun = Boolean(orgId && stream.trim() && hasRunnableQuery && !queryPending);
  const recoveryHasFilters = globalFilters.length > 0 || (
    queryMode === 'fields'
      ? fieldQuery.trim().length > 0
      : query.trim() !== defaultLogQuery(stream.trim())
  );
  const clearRecoveryFilters = React.useCallback(() => {
    clearGlobalFilters();
    if (queryMode === 'fields') setFieldQuery('');
    else setQuery(stream.trim() ? defaultLogQuery(stream.trim()) : '');
    setSelectedRow(null);
    setQueryError(null);
    // A fresh window object intentionally triggers the existing query effect
    // after React has committed the cleared local and global filters.
    setTimeWindow({ ...timeWindow });
  }, [clearGlobalFilters, queryMode, setFieldQuery, setQuery, setTimeWindow, stream, timeWindow]);
  const widenRecoveryRange = React.useCallback(() => {
    setTimeWindow(widenTimeWindow(timeWindow));
  }, [setTimeWindow, timeWindow]);
  const queryDocsHref = `https://docs.molesignal.com/${
    (i18n.resolvedLanguage ?? i18n.language).toLowerCase().startsWith('zh') ? 'zh' : 'en'
  }/query/logs`;

  React.useEffect(() => {
    if (previousTimeWindowRef.current === timeWindow) return;
    previousTimeWindowRef.current = timeWindow;
    if (!orgId || !stream.trim() || !hasRunnableQuery || queryPending) return;
    void executeQuery();
  }, [executeQuery, hasRunnableQuery, orgId, queryPending, stream, timeWindow]);

  const selectedLog = selectedRow === null ? null : viewRows[selectedRow] ?? null;
  // What a link to this query carries of the page's own: the stream and the statement, in the language it is in.
  const shareSearch = React.useMemo(() => {
    const params = new URLSearchParams();
    const statement = (queryMode === 'sql' ? query : fieldQuery).trim();

    if (stream) params.set('stream', stream);
    if (statement) params.set(queryMode === 'sql' ? 'sql' : 'q', statement);

    return params;
  }, [fieldQuery, query, queryMode, stream]);
  useQueryTabStatus({
    running: queryPending,
    summary: logQuerySummary({ stream, queryMode, query, fieldQuery }),
  });

  return (
    <div
      data-workspace="logs"
      className="flex h-full min-h-0 flex-col overflow-hidden bg-[var(--page-canvas)]"
    >
      {/* No title row: the explorer gives that row to the data and carries its
          name at the start of the workbench bar instead. */}
      <QueryWorkbench
        appearance="surface"
        className="mt-[6px] shrink-0"
        inlineBody={queryEditorCollapsed}
        title={<WorkbenchTitle title={t('explore.title')} />}
        toolbar={
          <>
            <QueryModeSwitch
              mode={editorMode}
              onChange={(next) => {
                setEditorMode(next);
                setQueryEditorCollapsed(false);
              }}
            />
            <QuerySyntaxHelp
              mode={queryMode}
              scope="logs"
              compact
              className="border-0"
            />
            <LogStreamSelect
              streams={streams}
              value={stream}
              loading={streamsQuery.isLoading}
              onChange={setAutoStream}
            />
          </>
        }
        actions={
          <>
            <TimeRangeChip className="border-0" />
            <QueryRunAction
              label={t('explore.toolbar.run')}
              runningLabel={t('explore.toolbar.running')}
              running={queryPending}
              onClick={() => void executeQuery()}
              disabled={!canRun}
            />
            <QueryIconAction
              icon={RefreshCw}
              label={t('explore.toolbar.refresh_aria')}
              onClick={() => void executeQuery()}
              disabled={!canRun}
            />
            <QueryShareButton search={shareSearch} />
          </>
        }
      >
        <QueryEditorFrame
          queryRef="A"
          value={queryMode === 'sql' ? query : fieldQuery}
          // One editor for both languages: what is typed says which it is.
          onChange={setCodeStatement}
          onClear={clearActiveQuery}
          clearLabel={t('explore.toolbar.clear_query')}
          // The shortcut belongs to the editor; the builder runs on Enter in its text box.
          {...(editorMode === 'code'
            ? { onModEnter: () => { if (canRun) void executeQuery(); } }
            : {})}
          language={queryMode === 'sql' ? 'sql' : 'field-query'}
          // One name whatever is typed: the mode is on the switch beside it, the language under the editor.
          label={tCommon('query_language.label')}
          languageBadge={false}
          ariaLabel={queryMode === 'sql' ? 'SQL query editor' : 'Log field query editor'}
          placeholder={logQueryPlaceholder(stream)}
          collapsed={queryEditorCollapsed}
          onCollapsedChange={setQueryEditorCollapsed}
          collapseLabel={t('explore.toolbar.collapse_editor')}
          expandLabel={t('explore.toolbar.expand_editor')}
          summary={activeQueryText || t('explore.toolbar.empty_query_summary')}
          completionItems={queryMode === 'fields' ? completionItems : sqlFunctions}
          frameClassName="border-0 bg-[var(--control-surface)] shadow-none"
          minHeight={112}
          maxHeight={420}
          lineNumbers
          resizable
          body={editorMode === 'builder' ? (
            <FilterBuilder
              statement={fieldQuery}
              onStatementChange={setBuilderStatement}
              dialect={logBuilderDialect}
              fields={builderFields}
              suggestValues={suggestBuilderValues}
              pendingField={builderField}
              onPendingFieldHandled={() => setBuilderField(null)}
              onRun={() => {
                if (canRun) void executeQuery();
              }}
              notice={queryMode === 'sql' ? tCommon('query_builder.sql_active') : undefined}
            />
          ) : undefined}
        />
      </QueryWorkbench>

      {/* Body: independent field and result scrollers fill the remaining viewport. */}
      <div
        data-log-workspace-layout="surface-grid"
        className="flex min-h-0 flex-1 gap-[8px] overflow-hidden bg-[var(--page-canvas)] pl-[var(--pane-gutter-start,20px)] pr-[var(--pane-gutter-end,20px)]"
      >
        <LogFieldPanel
          collapsed={fieldPanelCollapsed}
          onCollapsedChange={setFieldPanelCollapsed}
          fields={fields}
          visibleFields={visibleLogFields}
          queryMode={queryMode}
          records={rawRecords}
          moments={rowMoments}
          loading={queryPending && !queryResult}
          hasStream={Boolean(stream)}
          canRefresh={canRun}
          onRefresh={() => void executeQuery()}
          expandedField={expandedField}
          onExpandedFieldChange={setExpandedField}
          onToggleVisibility={toggleLogFieldVisibility}
          onInsertFilter={insertLogFieldFilter}
          onInsertValueFilter={insertLogFieldValueFilter}
          counted={countedQuery}
          range={executedTimeRange}
          maskedFields={maskedFields}
        />

        <div className="flex min-w-0 flex-1 overflow-hidden">
          <div
            data-workspace-pane="log-results"
            className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-md bg-[var(--functional-surface)]"
          >
            <LogResultSummary
              eventCount={viewRows.length}
              tookMs={queryResult?.took_ms ?? 0}
              timeWindowLabel={formatTimeWindowLabel(timeWindow, tCommon)}
              pending={queryPending}
              fields={fields.map((field) => field.name)}
              visibleFields={visibleLogFields}
              onToggleField={toggleLogFieldVisibility}
              onReorderField={reorderLogField}
              columnsArranged={logColumns.arranged}
              onResetColumns={logColumns.reset}
              histogramOpen={showHistogram}
              onHistogramOpenChange={setShowHistogram}
              onDownloadCsv={downloadVisibleCsv}
              onDownloadLog={downloadVisibleLog}
              views={<LogResultViewTabs value={view} onChange={setView} />}
              listControls={view === 'raw'}
              resultControls={view === 'raw' || view === 'patterns'}
            />
            {/* Without rows there is nothing to draw: the band would only be an empty
                strip pushing the empty state down. */}
            {showHistogram && rows.length > 0 && (view === 'raw' || view === 'patterns') && (
              <ResultHistogram
                points={histogramPoints}
                range={executedTimeRange}
                counts={histogramCounts}
                short={viewRows.length < 5}
                labels={{
                  chart: t('explore.toolbar.histogram'),
                  ok: t('explore.histogram.normal'),
                  errors: t('explore.histogram.errors'),
                }}
                onRangeSelect={selectHistogramRange}
              />
            )}

            <div className="min-h-0 flex-1 overflow-hidden">
              {queryError ? (
                <QueryRecoveryState
                  state="error"
                  error={queryError}
                  copy={{
                    errorTitle: t('explore.recovery.query_failed'),
                    emptyTitle: t('explore.recovery.no_data_title'),
                    emptyDescription: t('explore.recovery.no_data_description'),
                    clearFiltersLabel: t('explore.recovery.clear_filters'),
                    widenRangeLabel: t('explore.recovery.expand_time'),
                    docsLabel: t('explore.recovery.query_docs'),
                  }}
                  recovery={{
                    hasFilters: recoveryHasFilters,
                    onRetry: () => void executeQuery(),
                    onClearFilters: clearRecoveryFilters,
                    onWidenRange: widenRecoveryRange,
                    docsHref: queryDocsHref,
                  }}
                  className="h-full min-h-0 justify-center rounded-none border-0"
                  testId="logs-query-error"
                />
              ) : queryPending && !queryResult ? (
                <QueryState
                  state="loading"
                  loadingLabel={t('explore.table.running_query')}
                  className="h-full min-h-0"
                />
              ) : view === 'inspection' ? (
                // Shown without results too: a query that matches nothing now can still be
                // saved to be inspected on a schedule, and the saved inspections are reached
                // from here.
                <LogInspectionView
                  target={inspectionTarget}
                  rangeLabel={formatTimeWindowLabel(timeWindow, tCommon)}
                  rows={viewRows}
                  empty={rows.length === 0}
                  inspection={inspection}
                />
              ) : rows.length === 0 ? (
                <QueryRecoveryState
                  state="empty"
                  copy={{
                    errorTitle: t('explore.recovery.query_failed'),
                    emptyTitle: t('explore.recovery.no_data_title'),
                    emptyDescription: t('explore.recovery.no_data_description'),
                    clearFiltersLabel: t('explore.recovery.clear_filters'),
                    widenRangeLabel: t('explore.recovery.expand_time'),
                    docsLabel: t('explore.recovery.query_docs'),
                  }}
                  recovery={{
                    hasFilters: recoveryHasFilters,
                    onRetry: () => void executeQuery(),
                    onClearFilters: clearRecoveryFilters,
                    onWidenRange: widenRecoveryRange,
                    docsHref: queryDocsHref,
                  }}
                  className="h-full min-h-0"
                  testId="logs-no-data"
                />
              ) : view === 'stats' ? (
                <LogStatsView
                  stream={stream}
                  fields={fields}
                  counted={countedQuery}
                  range={executedTimeRange}
                  written={writtenStatement && queryResult ? { statement: writtenStatement, result: queryResult } : null}
                  timeWindow={timeWindow}
                  onOpenStatement={openStatement}
                />
              ) : view === 'patterns' ? (
                // What the loaded logs say, kind by kind, in place of the logs one by one.
                <LogPatternsView
                  rows={viewRows}
                  selectedIndex={selectedRow}
                  sample={
                    patternQuery
                      ? {
                          held: sampled,
                          reading: patternSample.reading,
                          more: patternSample.more,
                          failed: patternSample.failed,
                          step: patternQuery.step,
                          onReadMore: patternSample.readMore,
                          onRetry: patternSample.retry,
                        }
                      : undefined
                  }
                  onOpenLog={setSelectedRow}
                  onFilter={insertLogPatternFilter}
                />
              ) : (
                <LogListResults
                  rows={pagedRows}
                  fields={visibleLogFields}
                  startIndex={resultPageStart}
                  selectedIndex={selectedRow}
                  onSelect={setSelectedRow}
                />
              )}
            </div>
            {view !== 'raw' ? null : cursorResults ? (
              <CursorPagination
                className="border-t-0 bg-transparent"
                pageSize={logPageSize}
                pageSizeOptions={EXPLORER_PAGE_SIZE_OPTIONS}
                hasPrevious={Boolean(logCursorPage?.previous_cursor)}
                hasNext={Boolean(logCursorPage?.next_cursor)}
                pending={queryPending}
                ariaLabel={t('explore.table.pagination_aria')}
                pageSizeAriaLabel={t('explore.table.events_per_page_aria')}
                previousLabel={t('explore.table.prev_page')}
                nextLabel={t('explore.table.next_page')}
                onPrevious={() => changeLogPage('previous')}
                onNext={() => changeLogPage('next')}
                onPageSizeChange={changeLogPageSize}
              />
            ) : (
              <ResultPagination
                className="border-t-0 bg-transparent"
                page={activeResultPage}
                pageCount={resultPageCount}
                pageSize={resultPageSize}
                pageSizeOptions={resultPageSizes}
                pageLabel={t('explore.table.page_summary', {
                  page: activeResultPage,
                  pages: resultPageCount,
                })}
                ariaLabel={t('explore.table.pagination_aria')}
                pageSizeAriaLabel={t('explore.table.events_per_page_aria')}
                firstAriaLabel={t('explore.table.first_page_aria')}
                previousAriaLabel={t('explore.table.prev_page_aria')}
                nextAriaLabel={t('explore.table.next_page_aria')}
                lastAriaLabel={t('explore.table.last_page_aria')}
                onPageChange={changeResultPage}
                onPageSizeChange={changeResultPageSize}
              />
            )}
          </div>

          {selectedLog && selectedRow !== null && (
            <>
              {/* The page under the drawer is one click away: on another log that click opens it,
                  anywhere else it closes the drawer. */}
              <button
                type="button"
                aria-label={t('explore.detail.close')}
                tabIndex={-1}
                onClick={(event) => {
                  const row = document
                    .elementsFromPoint(event.clientX, event.clientY)
                    .find((element) => element.matches(LOG_OPENERS));

                  if (row instanceof HTMLElement) row.click();
                  else closeLogDetail();
                }}
                className="fixed bottom-0 left-0 right-0 top-topbar z-[55] cursor-default border-0 bg-transparent p-0 focus:outline-none"
              />
              <aside
                aria-label={t('explore.detail.drawer_aria')}
                className="fixed bottom-0 right-0 top-topbar z-[60] min-h-0 w-full max-w-[660px] border-l border-bd-1 bg-[var(--functional-surface)] shadow-drawer data-[state=open]:animate-slide-in-right sm:w-[34vw] sm:min-w-[440px]"
                data-state="open"
              >
                <LogDetail
                  log={selectedLog}
                  index={selectedRow}
                  total={viewRows.length}
                  stream={stream}
                  contextRows={viewRows.slice(
                    Math.max(0, selectedRow - 5),
                    Math.min(viewRows.length, selectedRow + 6),
                  )}
                  visibleFields={visibleLogFields}
                  onInsertValueFilter={insertLogFieldValueFilter}
                  onToggleVisibility={toggleLogFieldVisibility}
                  onSelectContext={(log) => {
                    const nextIndex = viewRows.indexOf(log);
                    if (nextIndex >= 0) setSelectedRow(nextIndex);
                  }}
                  onClose={closeLogDetail}
                  onPrev={() => setSelectedRow(Math.max(0, selectedRow - 1))}
                  onNext={() => setSelectedRow(Math.min(viewRows.length - 1, selectedRow + 1))}
                />
              </aside>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
