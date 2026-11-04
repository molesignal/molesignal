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
import { useFoldedWhileNarrow, useQueryTabState, useQueryTabStatus, useQueryTabStrip } from '@/shell/queryTabs';
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
import { logQueryCompletions } from './completions';
import { type PinnedLogFilter, pinnedLogFilters } from './conditions/model';
import { LogConditionNotice } from './conditions/Notice';
import { runLogCursorQuery } from './cursorQuery';
import { LogDetail } from './detail/Drawer';
import { LogDetailFrame } from './detail/Frame';
import {
  appendLogFieldClause,
  appendLogFieldContainsClause,
  appendLogFieldValueClause,
  buildLogFieldQuerySql,
  deriveLogFields,
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
  writeLogQueryParams,
} from './queryState';
import { LogResultSummary } from './ResultSummary';
import { LogListResults } from './ResultViews';
import {
  appendPinnedFilters,
  appendSqlFieldContainsFilter,
  appendSqlFieldFilter,
  appendSqlFieldValueFilter,
} from './sqlStatement';
import { LogStatsView } from './stats/View';
import { LogStreamSelect } from './StreamSelect';
import {
  logLevelTone,
  recordsToCsv,
  recordsToLogText,
  topLogFieldValues,
  type LogEntry,
  type LogLevel,
} from './viewModel';
import { LogResultViewTabs, useLogResultView } from './views';

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

const NO_FIELDS: readonly streamsApi.StreamField[] = [];
const NO_PINNED: readonly PinnedLogFilter[] = [];
const NO_FILTERS: readonly GlobalFilter[] = [];

/** What opens a log in the drawer: a row of the list, or a log under a pattern. */
const LOG_OPENERS = '[data-log-result-row="logs"], [data-log-pattern-example]';

export function Logs() {
  const { t, i18n } = useTranslation('logs');
  const { t: tCommon } = useTranslation('common');
  const fmt = useTimeFormatter();
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
  // Known once the list is in: a link's conditions and the pinned filters are applied on the
  // stream's own fields, and a stream the list does not have has none.
  const streamsListed = !streamsQuery.isPending;
  const fieldsOf = React.useCallback(
    (name: string) => {
      if (!streamsListed) return undefined;
      const listed = streams.find((candidate) => candidate.name === name && candidate.streamType === 'logs');
      return listed?.schema.fields ?? NO_FIELDS;
    },
    [streams, streamsListed],
  );
  const {
    editorMode,
    setEditorMode,
    setCodeStatement,
    setBuilderStatement,
    requestedStream,
    requestedFieldQuery,
    requestedLink,
    linkPending,
    linkGaps,
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
  } = useLogQueryState(fieldsOf);
  const selectedStream = React.useMemo(
    () => streams.find((candidate) => candidate.name === stream && candidate.streamType === 'logs'),
    [stream, streams],
  );
  const selectedStreamFields = React.useMemo(
    () => selectedStream?.schema.fields ?? [],
    [selectedStream],
  );
  // A field picked in the field list while the builder is open starts a filter there.
  const [builderField, setBuilderField] = React.useState<string | null>(null);
  // Which view of the results is shown: the logs, their statistics, an inspection, or their patterns.
  const [view, setView] = useLogResultView();
  // The histogram can be closed to give the rows more room; that choice is
  // remembered, so the page opens the way it was left.
  const showHistogram = useExplorerLayoutStore((state) => state.logsHistogramVisible);
  const setShowHistogram = useExplorerLayoutStore((state) => state.setLogsHistogramVisible);
  // Folded in a narrow window, and for as long as the page has a narrow share of a split one.
  const [fieldPanelCollapsed, setFieldPanelCollapsed] = useFoldedWhileNarrow(isNarrowViewport);
  // The editor starts folded into the bar, as on Traces: it opens when there is a
  // query to look at (one arrived in the link, a filter was added) or on request.
  // Kept with the tab: its address carries the query it ran, which is no reason to open it on a reload.
  const [queryEditorCollapsed, setQueryEditorCollapsed] = useQueryTabState(
    'logs.editorFolded',
    !requestedFieldQuery && !requestedSql && !requestedLink,
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

  const orgId = useAuthStore((s) => s.ctx?.orgId ?? '');
  const timeWindow = useTimeStore((s) => s.window);
  const setTimeWindow = useTimeStore((s) => s.setWindow);
  const globalFilters = useFiltersStore((s) => s.filters);
  const clearGlobalFilters = useFiltersStore((s) => s.clearFilters);
  // The pinned filters as they apply to the stream, on its own fields. One it keeps no field
  // for is not applied, and the page says so.
  const pinnedFilters = React.useMemo(
    () => (streamsListed ? pinnedLogFilters(globalFilters, selectedStreamFields) : null),
    [globalFilters, selectedStreamFields, streamsListed],
  );
  const appliedPinned = pinnedFilters?.applied ?? NO_PINNED;
  // The logs are asked for once what the query is made of is known: the link's conditions
  // written, and the fields the pinned filters apply on.
  const conditionsReady = !linkPending && (pinnedFilters !== null || globalFilters.length === 0);
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
    // The address says what runs: a copy of it is this query. Another page of it is the same query.
    if (!options.cursor) {
      publishQuery({ stream: selectedStream, mode: queryMode, statement: queryMode === 'sql' ? query : fieldQuery });
    }
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
      const statement = appendPinnedFilters(baseStatement, selectedStream, appliedPinned);
      let result: QueryResult;
      if (queryMode === 'fields') {
        const { page, result: cursorResult } = await runLogCursorQuery({
          stream: selectedStream,
          statement: fieldQuery,
          pinned: appliedPinned,
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
        result = await queryApi.readQuery({
          orgId: orgId,
          language: 'sql',
          statement,
          timeRange: queryTimeRange,
          stream: { name: selectedStream, streamType: 'logs' },
          limit: DEFAULT_LOG_LIMIT,
        });
      }
      setQueryResult(result);
      setExecutedTimeRange(queryTimeRange);
      // Another page is of the same query: what it matches has been counted.
      if (!options.cursor) {
        setCountedQuery(
          queryMode === 'fields'
            ? { orgId, stream: selectedStream, statement: fieldQuery, pinned: appliedPinned, ranAt: Date.now() }
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
    appliedPinned,
    executedTimeRange,
    fieldQuery,
    logPageSize,
    orgId,
    publishQuery,
    query,
    queryMode,
    resetLogCursor,
    stream,
    timeWindow,
  ]);

  React.useEffect(() => {
    if (!requestedStream || requestedStream === addressTaken.current.stream) return;
    addressTaken.current.stream = requestedStream;
    if (requestedStream === stream) return;
    setAutoStream(requestedStream, { preserveFieldQuery: Boolean(requestedFieldQuery) });
  }, [addressTaken, requestedFieldQuery, requestedStream, setAutoStream, stream]);

  React.useEffect(() => {
    if (stream || streams.length === 0) return;
    // Streams are queried by name (`stream: { name }`), so the default
    // selection must be the stream *name* — using `.id` here makes the backend
    // reject with `stream not found: <id>`.
    setAutoStream(streams[0]!.name, { preserveFieldQuery: Boolean(requestedFieldQuery) });
  }, [requestedFieldQuery, setAutoStream, stream, streams]);

  // A query the address brings (a link, the history) is shown from the start in an open editor,
  // and runs: what was on screen answered another one.
  const openArrivedQuery = React.useCallback(() => {
    setSelectedRow(null);
    setResultPage(1);
    setQueryResult(undefined);
    setLogCursorPage(null);
    setQueryError(null);
    setQueryEditorCollapsed(false);
    initialRunStreamRef.current = null;
  }, [setQueryEditorCollapsed]);

  React.useEffect(() => {
    if (requestedFieldQuery === addressTaken.current.fieldQuery) return;
    addressTaken.current.fieldQuery = requestedFieldQuery;
    setQueryMode('fields');
    setFieldQuery(requestedFieldQuery);
    openArrivedQuery();
  }, [addressTaken, openArrivedQuery, requestedFieldQuery, setFieldQuery, setQueryMode]);

  // Apply the raw-SQL seed (`?sql=`) with the same state reset as a field query,
  // then enter SQL mode with the statement verbatim and let auto-run fire.
  React.useEffect(() => {
    if (!requestedSql || requestedSql === addressTaken.current.sql) return;
    addressTaken.current.sql = requestedSql;
    setQueryMode('sql');
    setQuery(requestedSql);
    openArrivedQuery();
  }, [addressTaken, openArrivedQuery, requestedSql, setQuery, setQueryMode]);

  // A link's conditions are written into the field query once the stream's fields are
  // known (`useLogQueryState`): that is the query the page opens on, and it runs.
  const linkPendingRef = React.useRef(linkPending);
  React.useEffect(() => {
    const written = linkPendingRef.current && !linkPending;
    linkPendingRef.current = linkPending;
    if (written) openArrivedQuery();
  }, [linkPending, openArrivedQuery]);

  React.useEffect(() => {
    const runKey = `${orgId}:${stream}`;
    if (!orgId || !stream || !conditionsReady || (queryMode === 'sql' && !query.trim())) return;
    if (initialRunStreamRef.current === runKey) return;
    initialRunStreamRef.current = runKey;
    void executeQuery();
    // Runs once for the active org + stream. Re-runs are explicit through the
    // query / refresh controls.
  }, [conditionsReady, executeQuery, orgId, query, queryMode, stream]);

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
      ? logCursorPage?.previousCursor
      : logCursorPage?.nextCursor;
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
    () => logQueryCompletions(sqlFunctions, fields, rows, maskedFields),
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
  }, [editorMode, fields, queryMode, setFieldQuery, setQuery, setQueryEditorCollapsed, stream]);

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
  }, [editorMode, fields, queryMode, setFieldQuery, setQuery, setQueryEditorCollapsed, setQueryMode, stream]);

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
  }, [editorMode, queryMode, setFieldQuery, setQuery, setQueryEditorCollapsed, setQueryMode, stream]);

  // A statement the statistics view wrote goes into the editor as the reader's own, and is run:
  // the run-once guard is lifted, so the effect below runs it with the state as it then is.
  const openStatement = React.useCallback((statement: string) => {
    setQueryMode('sql');
    setQuery(statement);
    setQueryEditorCollapsed(false);
    initialRunStreamRef.current = null;
  }, [setQuery, setQueryEditorCollapsed, setQueryMode]);
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
  const canRun = Boolean(orgId && stream.trim() && hasRunnableQuery && !queryPending && conditionsReady);
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
    (i18n.resolvedLanguage ?? i18n.language).toLowerCase().startsWith('zh') ? 'zh-Hans' : 'en-US'
  }/logs`;

  React.useEffect(() => {
    if (previousTimeWindowRef.current === timeWindow) return;
    previousTimeWindowRef.current = timeWindow;
    if (!orgId || !stream.trim() || !hasRunnableQuery || queryPending || !conditionsReady) return;
    void executeQuery();
  }, [conditionsReady, executeQuery, hasRunnableQuery, orgId, queryPending, stream, timeWindow]);

  const selectedLog = selectedRow === null ? null : viewRows[selectedRow] ?? null;
  // What a link to this query carries of the page's own: the stream, the statement in the language
  // it is in, and the view of its results.
  const shareSearch = React.useMemo(() => {
    const statement = queryMode === 'sql' ? query : fieldQuery;
    const params = writeLogQueryParams(new URLSearchParams(), { stream, mode: queryMode, statement });

    if (view !== 'raw') params.set('view', view);

    return params;
  }, [fieldQuery, query, queryMode, stream, view]);
  useQueryTabStatus({
    running: queryPending,
    summary: logQuerySummary({ stream, queryMode, query, fieldQuery }),
  });
  const queryTabStrip = useQueryTabStrip();

  return (
    <div
      data-workspace="logs"
      className="flex h-full min-h-0 flex-col overflow-hidden bg-[var(--page-canvas)]"
    >
      {/* No title row: the explorer gives that row to the data and carries its
          name at the start of the workbench bar instead. Its query tabs are in
          the query box, so the bar is the first card under the top bar, as far
          off it as the cards are off each other. */}
      <QueryWorkbench
        appearance="surface"
        className="mt-[12px] shrink-0"
        inlineBody={queryEditorCollapsed}
        notice={
          // Said only of a stream the list has: without its fields, nothing is known to be missing from it.
          selectedStream && (linkGaps.length > 0 || (pinnedFilters?.unmatched.length ?? 0) > 0) ? (
            <LogConditionNotice
              stream={selectedStream.name}
              link={linkGaps}
              pinned={pinnedFilters?.unmatched ?? NO_FILTERS}
            />
          ) : undefined
        }
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
          tabs={queryTabStrip}
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
      <div className="flex min-h-0 flex-1 overflow-hidden bg-[var(--page-canvas)] pl-[var(--pane-gutter-start,20px)] pr-[var(--pane-gutter-end,20px)]">
        {/* The field list and the results are one surface, as in the trace explorer: the list is the margin of the results. */}
        <div
          data-log-workspace-layout="joined"
          className="flex min-h-0 min-w-0 flex-1 items-stretch overflow-hidden rounded-md bg-[var(--functional-surface)]"
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
            <div data-workspace-pane="log-results" className="flex min-w-0 flex-1 flex-col overflow-hidden">
              <LogResultSummary
                eventCount={viewRows.length}
                tookMs={queryResult?.tookMs ?? 0}
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
                  hasPrevious={Boolean(logCursorPage?.previousCursor)}
                  hasNext={Boolean(logCursorPage?.nextCursor)}
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
                <LogDetailFrame label={t('explore.detail.drawer_aria')}>
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
                </LogDetailFrame>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
