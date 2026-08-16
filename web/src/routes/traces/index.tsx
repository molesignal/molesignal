import { useQuery } from '@tanstack/react-query';
import dayjs from 'dayjs';
import {
  Play,
  RefreshCw,
  X,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useSearchParams } from 'react-router-dom';

import * as queryApi from '@/api/query';
import * as streamsApi from '@/api/streams';
import * as webApi from '@/api/web';
import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { useTimeFormatter } from '@/lib/time';
import type { CursorPage } from '@/pagination/cursor';
import { useCursorPagination } from '@/pagination/useCursorPagination';
import {
  Card,
  CardBody,
  CardHeader,
  ChromeButton,
  TimeRangeChip,
  uiLabelClass,
} from '@/shell/chrome';
import type { CodeCompletionItem } from '@/shell/codeEditor/types';
import { EmptyState } from '@/shell/EmptyState';
import { PageHeader } from '@/shell/PageHeader';
import { QueryEditorFrame } from '@/shell/query/EditorFrame';
import {
  QueryRecoveryState,
  type QueryRecoveryActions,
  type QueryRecoveryCopy,
} from '@/shell/query/RecoveryState';
import { queryStateFor } from '@/shell/query/State';
import { QuerySyntaxHelp } from '@/shell/query/SyntaxHelp';
import { useSqlFunctionCompletions } from '@/shell/query/useSqlFunctionCompletions';
import {
  QueryToolbarButton,
  QueryToolbarGroup,
  QueryToolbarTabs,
  QueryWorkbench,
  type QueryToolbarTab,
} from '@/shell/query/Workbench';
import { SignalReference, type SignalReferenceType } from '@/shell/SignalReference';
import { TimezoneSelect } from '@/shell/TimezoneSelect';
import { Button } from '@/shell/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import { Tooltip, TooltipContent, TooltipPortal, TooltipTrigger } from '@/shell/ui/tooltip';
import { useAuthStore } from '@/stores/auth';
import { resolveWindow, type TimeWindow, useTimeStore } from '@/stores/useTimeStore';
import type { QueryResult } from '@/types/query';
import { formatTraceDurationMs } from '@/viz/trace/duration';
import { TraceFlame } from '@/viz/trace/TraceFlame';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { traceFieldValue, type TraceFieldRecord } from './fieldPanel/model';
import { TraceFieldPanel } from './fieldPanel/Panel';
import {
  appendTraceSqlFieldFilter,
  DEFAULT_VISIBLE_TRACE_FIELDS,
  deriveTraceFields,
  insertTraceClause,
  parseTraceStatement,
  selectTraceStream,
  TRACE_RESULT_LIMIT,
  traceSqlPlaceholder,
  type ParsedTraceStatement,
  type TraceFieldDef,
  type TraceFieldName,
  type TraceQueryMode,
} from './fieldQueryModel';
import {
  DEFAULT_TRACE_PAGE_SIZE,
  TracePagination,
  type TracePaginationModel,
} from './Pagination';
import { ServiceGraphPanel } from './serviceGraph/Panel';
import {
  parseTraceListSort,
  TRACE_LIST_SORT_OPTIONS,
  writeTraceListSort,
} from './sort';
import {
  quoteTraceValue,
  traceUrlQueryStateFromParams,
  traceUrlQueryStateKey,
} from './urlState';

type DisplayTrace = TraceFieldRecord;

type TraceListData = CursorPage<DisplayTrace>;

type TraceTab = 'spans' | 'traces' | 'service-graph' | 'service-catalog';

const TRACE_DEFAULT_WINDOW: TimeWindow = {
  from: 'now-24h',
  to: 'now',
  mode: 'relative',
};

// Tabs / query-mode buttons / field hints carry i18n keys; the rendering
// component calls `t(labelKey)` so locale changes flow through without
// re-creating the array.
const TRACE_QUERY_MODES: Array<{ id: TraceQueryMode; labelKey: string }> = [
  { id: 'fields', labelKey: 'explore.query.modes.fields' },
  { id: 'sql', labelKey: 'explore.query.modes.sql' },
];

const TRACE_TABS: Array<{ id: TraceTab; labelKey: string }> = [
  { id: 'spans', labelKey: 'explore.tabs.spans' },
  { id: 'traces', labelKey: 'explore.tabs.traces' },
  { id: 'service-graph', labelKey: 'explore.tabs.service_graph' },
  { id: 'service-catalog', labelKey: 'explore.tabs.service_catalog' },
];

function quotedTraceCompletion(value: string): string {
  return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

function traceQueryCompletions(fields: TraceFieldDef[], traces: DisplayTrace[]): CodeCompletionItem[] {
  const fieldNames = new Set<string>(fields.map((field) => field.name));
  const values = new Set<string>(['checkout', 'api', 'web', 'OK', 'ERROR', 'UNSET']);
  for (const trace of traces.slice(0, 200)) {
    if (trace.id) values.add(trace.id);
    if (trace.service) values.add(trace.service);
    if (trace.op) values.add(trace.op);
    values.add(trace.errors > 0 ? 'ERROR' : 'OK');
  }
  return [
    ...Array.from(fieldNames).sort().map((label) => ({ label, kind: 'field' as const, detail: 'trace field' })),
    { label: '=', insertText: '= ', kind: 'operator', detail: 'operator' },
    { label: '!=', insertText: '!= ', kind: 'operator', detail: 'operator' },
    { label: '>=', insertText: '>= ', kind: 'operator', detail: 'operator' },
    { label: '<=', insertText: '<= ', kind: 'operator', detail: 'operator' },
    { label: '>', insertText: '> ', kind: 'operator', detail: 'operator' },
    { label: '<', insertText: '< ', kind: 'operator', detail: 'operator' },
    { label: 'contains', insertText: 'contains ', kind: 'operator', detail: 'operator' },
    { label: 'AND', insertText: 'AND ', kind: 'operator', detail: 'operator' },
    { label: 'OR', insertText: 'OR ', kind: 'operator', detail: 'operator' },
    ...Array.from(values).sort().map((value) => {
      const quoted = quotedTraceCompletion(value);
      return { label: quoted, insertText: quoted, kind: 'value' as const, detail: 'value' };
    }),
  ];
}

function rangeFromWindow(window: TimeWindow, now: Date): { from: string; to: string } {
  const resolvedWindow = resolveWindow(window, now);
  return { from: resolvedWindow.from.toISOString(), to: resolvedWindow.to.toISOString() };
}

function tabFromParam(value: string | null): TraceTab | null {
  if (value === 'map') return 'service-graph';
  if (value === 'analytics') return 'service-catalog';
  return TRACE_TABS.some((tab) => tab.id === value) ? (value as TraceTab) : null;
}

function traceColumnIndex(result: QueryResult): Record<string, number> {
  return result.columns.reduce<Record<string, number>>((acc, column, index) => {
    acc[column.toLowerCase()] = index;
    return acc;
  }, {});
}

function traceCell(row: unknown[], index: Record<string, number>, names: string[]): unknown {
  for (const name of names) {
    const hit = index[name.toLowerCase()];
    if (hit !== undefined) return row[hit];
  }
  return undefined;
}

function stringTraceCell(row: unknown[], index: Record<string, number>, names: string[], fallback = '-'): string {
  const value = traceCell(row, index, names);
  return value === null || value === undefined || value === '' ? fallback : String(value);
}

function numberTraceCell(row: unknown[], index: Record<string, number>, names: string[], fallback = 0): number {
  const value = traceCell(row, index, names);
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string' && value.trim()) {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return fallback;
}

function traceSqlResultToDisplayTraces(result: QueryResult): DisplayTrace[] {
  const index = traceColumnIndex(result);
  return result.rows.map((row, rowIndex) => {
    const startNs = numberTraceCell(row, index, ['start_ns', 'start_time_unix_nano'], 0);
    const endNs = numberTraceCell(row, index, ['end_ns', 'end_time_unix_nano'], 0);
    const durationMs = numberTraceCell(
      row,
      index,
      ['duration_ms'],
      numberTraceCell(row, index, ['duration_us'], endNs > startNs ? (endNs - startNs) / 1000 : 0) / 1000,
    );
    const status = stringTraceCell(row, index, ['status_code', 'status'], 'OK').toUpperCase();
    return {
      id: stringTraceCell(row, index, ['trace_id', 'id'], `sql-row-${rowIndex + 1}`),
      op: stringTraceCell(row, index, ['operation', 'op', 'operation_name'], '-'),
      service: stringTraceCell(row, index, ['service', 'service_name'], '-'),
      startNs,
      durationMs,
      spans: Math.max(1, Math.round(numberTraceCell(row, index, ['span_count', 'spans', 'count'], 1))),
      errors: Math.max(0, Math.round(numberTraceCell(row, index, ['error_count', 'errors'], status === 'ERROR' ? 1 : 0))),
    };
  });
}

export function Traces() {
  const { t, i18n } = useTranslation('traces');
  const orgId = useAuthStore((s) => s.ctx?.org_id ?? '');
  const timeWindow = useTimeStore((s) => s.window);
  const setTimeWindow = useTimeStore((s) => s.setWindow);
  const [searchParams, setSearchParams] = useSearchParams();
  const applyTraceDefaultWindow = React.useRef(
    !searchParams.has('time')
      && !(searchParams.has('from') && searchParams.has('to'))
      && timeWindow.mode === 'relative'
      && timeWindow.from === 'now-1h'
      && timeWindow.to === 'now',
  );
  const shouldApplyTraceDefaultWindow = applyTraceDefaultWindow.current;
  const effectiveTimeWindow = shouldApplyTraceDefaultWindow
    ? TRACE_DEFAULT_WINDOW
    : timeWindow;
  const traceSort = parseTraceListSort(searchParams.get('sort'));
  const requestedTraceStream = searchParams.get('stream')?.trim() ?? '';
  const urlTabParam = searchParams.get('tab') ?? searchParams.get('view');
  const urlTab = tabFromParam(urlTabParam);
  const initialUrlQuery = React.useRef(
    traceUrlQueryStateFromParams(searchParams),
  ).current;
  const [tab, setTab] = React.useState<TraceTab>(() => urlTab ?? 'spans');
  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const [queryMode, setQueryMode] = React.useState<TraceQueryMode>(
    initialUrlQuery.mode,
  );
  const [queryDraft, setQueryDraft] = React.useState(initialUrlQuery.fields);
  const [queryText, setQueryText] = React.useState(initialUrlQuery.fields);
  const [sqlDraft, setSqlDraft] = React.useState(initialUrlQuery.sql);
  const [sqlText, setSqlText] = React.useState(initialUrlQuery.sql);
  const [fieldFilter, setFieldFilter] = React.useState('');
  const [fieldPanelCollapsed, setFieldPanelCollapsed] = React.useState(false);
  const [queryEditorCollapsed, setQueryEditorCollapsed] = React.useState(
    !initialUrlQuery.fields && !initialUrlQuery.sql,
  );
  const [visibleTraceFields, setVisibleTraceFields] = React.useState<TraceFieldName[]>(DEFAULT_VISIBLE_TRACE_FIELDS);
  const [tracePage, setTracePage] = React.useState(1);
  const [rangeRefreshAt, setRangeRefreshAt] = React.useState(() => Date.now());
  const appliedTraceQueryRef = React.useRef(
    traceUrlQueryStateKey(initialUrlQuery),
  );
  const appliedTabParamRef = React.useRef<string | null>(urlTabParam);

  React.useLayoutEffect(() => {
    applyTraceDefaultWindow.current = false;
    if (shouldApplyTraceDefaultWindow) {
      setTimeWindow(TRACE_DEFAULT_WINDOW);
    }
  }, [setTimeWindow, shouldApplyTraceDefaultWindow]);

  React.useEffect(() => {
    if (urlTabParam === appliedTabParamRef.current) return;
    appliedTabParamRef.current = urlTabParam;
    if (urlTab) setTab(urlTab);
  }, [urlTab, urlTabParam]);

  React.useEffect(() => {
    const next = traceUrlQueryStateFromParams(searchParams);
    const nextKey = traceUrlQueryStateKey(next);
    if (nextKey === appliedTraceQueryRef.current) return;
    appliedTraceQueryRef.current = nextKey;
    setQueryMode(next.mode);
    if (next.mode === 'sql') {
      setSqlDraft(next.sql);
      setSqlText(next.sql);
    } else {
      setQueryDraft(next.fields);
      setQueryText(next.fields);
    }
    setQueryEditorCollapsed(false);
    setTab('spans');
  }, [searchParams]);

  const range = React.useMemo(
    () => rangeFromWindow(effectiveTimeWindow, new Date(rangeRefreshAt)),
    [effectiveTimeWindow, rangeRefreshAt],
  );
  const traceCursorContextKey = React.useMemo(
    () => JSON.stringify([
      orgId,
      range.from,
      range.to,
      queryMode,
      queryText,
      traceSort,
    ]),
    [orgId, queryMode, queryText, range.from, range.to, traceSort],
  );
  const {
    cursor: traceCursor,
    pageSize: tracePageSize,
    reset: resetTraceCursor,
    goPrevious: goToPreviousTraceCursor,
    goNext: goToNextTraceCursor,
    setPageSize: setTraceCursorPageSize,
  } = useCursorPagination({
    contextKey: traceCursorContextKey,
    defaultPageSize: DEFAULT_TRACE_PAGE_SIZE,
  });

  const streamsQuery = useQuery({
    queryKey: ['streams', 'traces-fields'],
    queryFn: () => streamsApi.list(500),
    staleTime: 60_000,
  });
  const traceStreams = React.useMemo(
    () => (streamsQuery.data ?? []).filter((stream) => (
      stream.type === 'traces' && streamsApi.isQueryable(stream)
    )),
    [streamsQuery.data],
  );
  const primaryTraceDefinition = React.useMemo(
    () => selectTraceStream(traceStreams, requestedTraceStream),
    [requestedTraceStream, traceStreams],
  );
  const primaryTraceStream = primaryTraceDefinition?.name ?? '';
  const traceFields = React.useMemo(
    () => deriveTraceFields(primaryTraceDefinition?.schema.fields ?? []),
    [primaryTraceDefinition],
  );
  const parsedQuery = React.useMemo(() => (
    queryMode === 'fields'
      ? parseTraceStatement(queryText, traceFields)
      : { filters: [], rejected: [] }
  ), [queryMode, queryText, traceFields]);

  const topologyQuery = useQuery({
    queryKey: ['web', 'topology', range.from, range.to],
    queryFn: () => webApi.topology(range.from, range.to),
  });

  const traceListQuery = useQuery<TraceListData>({
    queryKey: [
      'web',
      'traces',
      range.from,
      range.to,
      queryMode,
      queryText,
      sqlText,
      traceSort,
      primaryTraceStream,
      orgId,
      traceCursor,
      tracePageSize,
    ],
    enabled: queryMode === 'fields'
      || Boolean(orgId && primaryTraceStream && sqlText.trim()),
    queryFn: async () => {
      if (queryMode === 'sql') {
        const statement = sqlText.trim();
        if (!statement || !primaryTraceStream) {
          return {
            items: [],
            next_cursor: null,
            previous_cursor: null,
            has_more: false,
          };
        }
        const result = await queryApi.runQuery({
          org_id: orgId,
          language: 'sql',
          statement,
          time_range: {
            start: Date.parse(range.from) * 1000,
            end: Date.parse(range.to) * 1000,
          },
          stream: { name: primaryTraceStream, stream_type: 'traces' },
          limit: TRACE_RESULT_LIMIT,
        });
        return {
          items: traceSqlResultToDisplayTraces(result),
          next_cursor: null,
          previous_cursor: null,
          has_more: false,
        };
      }
      const response = await webApi.traces(
        traceCursor
          ? {
              limit: tracePageSize,
              cursor: traceCursor,
            }
          : {
              from: Date.parse(range.from) * 1000,
              to: Date.parse(range.to) * 1000,
              limit: tracePageSize,
              ...(parsedQuery.q ? { q: parsedQuery.q } : {}),
              ...(parsedQuery.filters.length > 0
                ? { filters: parsedQuery.filters }
                : {}),
              sort: traceSort,
            },
      );
      return {
        ...response,
        items: (response.items ?? []).map((item) => ({
          id: item.trace_id,
          op: item.operation,
          service: item.service,
          startNs: item.start_ns,
          durationMs: item.duration_ms,
          spans: item.span_count,
          errors: item.error_count,
        })),
      };
    },
  });

  const traceList = React.useMemo(
    () => traceListQuery.data?.items ?? [],
    [traceListQuery.data?.items],
  );
  const tracePageCount = queryMode === 'sql'
    ? Math.max(1, Math.ceil(traceList.length / tracePageSize))
    : 1;
  const activeTracePage = Math.min(tracePage, tracePageCount);
  const tracePageStart = (activeTracePage - 1) * tracePageSize;
  const pagedTraceList = React.useMemo(
    () => queryMode === 'fields'
      ? traceList
      : traceList.slice(tracePageStart, tracePageStart + tracePageSize),
    [queryMode, traceList, tracePageSize, tracePageStart],
  );

  React.useEffect(() => {
    setTracePage((current) => Math.min(current, tracePageCount));
  }, [tracePageCount]);

  React.useEffect(() => {
    setTracePage(1);
  }, [queryMode, queryText, range.from, range.to, sqlText, traceSort]);

  const changeTracePage = React.useCallback((nextPage: number) => {
    setTracePage(Math.min(Math.max(1, nextPage), tracePageCount));
    setSelectedId(null);
  }, [tracePageCount]);

  const changeTracePageSize = React.useCallback((nextPageSize: number) => {
    setTraceCursorPageSize(nextPageSize);
    setTracePage(1);
    setSelectedId(null);
  }, [setTraceCursorPageSize]);

  const showPreviousTracePage = React.useCallback(() => {
    goToPreviousTraceCursor(traceListQuery.data);
    setSelectedId(null);
  }, [goToPreviousTraceCursor, traceListQuery.data]);

  const showNextTracePage = React.useCallback(() => {
    goToNextTraceCursor(traceListQuery.data);
    setSelectedId(null);
  }, [goToNextTraceCursor, traceListQuery.data]);

  const changeTraceSort = React.useCallback((nextSort: webApi.TraceListSort) => {
    setSearchParams(writeTraceListSort(searchParams, nextSort), { replace: true });
    resetTraceCursor();
    setTracePage(1);
    setSelectedId(null);
  }, [resetTraceCursor, searchParams, setSearchParams]);

  const tracePaginationModel = React.useMemo<TracePaginationModel>(
    () => queryMode === 'fields'
      ? {
          kind: 'cursor',
          pageSize: tracePageSize,
          hasPrevious: Boolean(traceListQuery.data?.previous_cursor),
          hasNext: Boolean(traceListQuery.data?.next_cursor),
          pending: traceListQuery.isFetching,
          onPrevious: showPreviousTracePage,
          onNext: showNextTracePage,
          onPageSizeChange: changeTracePageSize,
        }
      : {
          kind: 'offset',
          page: activeTracePage,
          pageCount: tracePageCount,
          pageSize: tracePageSize,
          onPageChange: changeTracePage,
          onPageSizeChange: changeTracePageSize,
        },
    [
      activeTracePage,
      changeTracePage,
      changeTracePageSize,
      queryMode,
      showNextTracePage,
      showPreviousTracePage,
      traceListQuery.data?.next_cursor,
      traceListQuery.data?.previous_cursor,
      traceListQuery.isFetching,
      tracePageCount,
      tracePageSize,
    ],
  );

  // Labels panel fields come from the live traces-stream schema (standard-OTEL
  // dotted columns), mirroring how Logs derives fields from query columns. Empty
  // instance → no traces stream → `deriveTraceFields` falls back to a minimal set.
  const traceCompletionItems = React.useMemo(
    () => traceQueryCompletions(traceFields, traceList),
    [traceFields, traceList],
  );

  React.useEffect(() => {
    if (traceList.length === 0) {
      setSelectedId(null);
      return;
    }
    if (selectedId !== null && !traceList.some((trace) => trace.id === selectedId)) {
      setSelectedId(null);
    }
  }, [traceList, selectedId]);

  const selectedTrace = React.useMemo(
    () => traceList.find((trace) => trace.id === selectedId) ?? null,
    [selectedId, traceList],
  );

  const selectTab = React.useCallback((next: TraceTab) => {
    setTab(next);
    const params = new URLSearchParams(searchParams);
    params.delete('view');
    if (next === 'spans') params.delete('tab');
    else params.set('tab', next);
    appliedTabParamRef.current = params.get('tab') ?? params.get('view');
    setSearchParams(params, { replace: true });
  }, [searchParams, setSearchParams]);

  const applySearch = React.useCallback(() => {
    if (queryMode === 'sql') {
      setSqlText(sqlDraft.trim());
    } else {
      setQueryText(queryDraft.trim());
    }
    setQueryEditorCollapsed(true);
    if (tab === 'service-graph' || tab === 'service-catalog') selectTab('spans');
  }, [queryDraft, queryMode, selectTab, sqlDraft, tab]);

  const changeQueryMode = React.useCallback((nextMode: TraceQueryMode) => {
    setQueryMode(nextMode);
    setQueryEditorCollapsed(false);
  }, []);

  const toggleTraceField = React.useCallback((field: TraceFieldName) => {
    setVisibleTraceFields((current) => {
      if (current.includes(field)) return current.filter((item) => item !== field);
      return [...current, field];
    });
  }, []);

  const insertFieldFilter = React.useCallback((field: TraceFieldDef) => {
    if (queryMode === 'sql') {
      if (!primaryTraceStream) return;
      setSqlDraft((current) => appendTraceSqlFieldFilter(current, field, primaryTraceStream));
    } else {
      setQueryDraft((current) => insertTraceClause(current, field));
    }
    setQueryEditorCollapsed(false);
    selectTab('spans');
  }, [primaryTraceStream, queryMode, selectTab]);

  const listState = queryStateFor({
    isLoading: traceListQuery.isLoading,
    isError: traceListQuery.isError,
    data: traceList,
  });
  const traceQueryRunning = traceListQuery.isFetching;
  const traceQueryCanRun = queryMode === 'fields'
    || Boolean(orgId && primaryTraceStream && sqlDraft.trim());

  // Aggregate stats from topology (real data!)
  const allEdges = topologyQuery.data?.edges ?? [];
  const totalRps = allEdges.reduce((a, e) => a + e.rps, 0);
  const avgErrRate = allEdges.length > 0 ? allEdges.reduce((a, e) => a + e.err_rate, 0) / allEdges.length : 0;
  const maxP95 = allEdges.reduce((a, e) => Math.max(a, e.p95_ms), 0);
  const refreshCurrentView = React.useCallback(() => {
    if (timeWindow.mode === 'relative') {
      resetTraceCursor();
      setRangeRefreshAt(Date.now());
      return;
    }
    void topologyQuery.refetch();
    if (queryMode === 'fields' && traceCursor !== null) {
      resetTraceCursor();
    } else {
      void traceListQuery.refetch();
    }
  }, [
    queryMode,
    resetTraceCursor,
    traceCursor,
    timeWindow.mode,
    topologyQuery,
    traceListQuery,
  ]);
  const clearRecoveryFilters = React.useCallback(() => {
    const params = new URLSearchParams(searchParams);
    for (const key of [
      'q',
      'query',
      'sql',
      'trace_id',
      'traceId',
      'span_id',
      'spanId',
      'service',
      'service_name',
      'operation',
      'operation_name',
      'path',
      'route',
      'status',
      'status_code',
    ]) {
      params.delete(key);
    }
    appliedTraceQueryRef.current = traceUrlQueryStateKey({
      mode: 'fields',
      fields: '',
      sql: '',
    });
    setQueryMode('fields');
    setQueryDraft('');
    setQueryText('');
    setSqlDraft('');
    setSqlText('');
    resetTraceCursor();
    setSelectedId(null);
    setSearchParams(params, { replace: true });
  }, [resetTraceCursor, searchParams, setSearchParams]);
  const widenRecoveryRange = React.useCallback(() => {
    resetTraceCursor();
    setTimeWindow(widenTimeWindow(effectiveTimeWindow));
  }, [effectiveTimeWindow, resetTraceCursor, setTimeWindow]);
  const recoveryCopy: QueryRecoveryCopy = {
    errorTitle: t('explore.recovery.query_failed'),
    emptyTitle: t('explore.recovery.no_data_title'),
    emptyDescription: t('explore.recovery.no_data_description'),
    clearFiltersLabel: t('explore.recovery.clear_filters'),
    widenRangeLabel: t('explore.recovery.expand_time'),
    docsLabel: t('explore.recovery.query_docs'),
  };
  const recovery: QueryRecoveryActions = {
    hasFilters: Boolean(queryText.trim() || sqlText.trim()),
    onRetry: () => void traceListQuery.refetch(),
    onClearFilters: clearRecoveryFilters,
    onWidenRange: widenRecoveryRange,
    docsHref: `https://docs.molesignal.io/${
      (i18n.resolvedLanguage ?? i18n.language).toLowerCase().startsWith('zh') ? 'zh' : 'en'
    }/query/traces`,
  };

  return (
    <div
      data-workspace="traces"
      className="flex h-[calc(100vh-var(--topbar-h)-var(--contextbar-h,0px))] min-h-0 flex-col overflow-hidden bg-bg-0"
    >
      <PageHeader
        title={t('explore.title')}
        subtitle={t('explore.subtitle')}
        className="shrink-0"
      />

      <TraceQueryPanel
        tab={tab}
        tabs={TRACE_TABS.map((tb) => ({
          id: tb.id,
          label: t(tb.labelKey),
          count: tb.id === 'spans' ? (selectedTrace?.spans ?? 0) : tb.id === 'traces' ? traceList.length : undefined,
        }))}
        queryMode={queryMode}
        queryDraft={queryDraft}
        queryText={queryText}
        sqlDraft={sqlDraft}
        parsedQuery={parsedQuery}
        completionItems={traceCompletionItems}
        running={traceQueryRunning}
        canRun={traceQueryCanRun}
        collapsed={queryEditorCollapsed}
        sqlPlaceholder={traceSqlPlaceholder(primaryTraceStream)}
        onTabChange={selectTab}
        onQueryModeChange={changeQueryMode}
        onQueryDraftChange={setQueryDraft}
        onSqlDraftChange={setSqlDraft}
        onCollapsedChange={setQueryEditorCollapsed}
        onApplySearch={applySearch}
        onRefresh={refreshCurrentView}
      />

      {/* Query workspaces fill the remaining viewport. Result panes own their
          scrolling so empty states stretch vertically and pagination stays
          pinned to the bottom edge. */}
      <div
        className={
          tab === 'spans' || tab === 'traces' || tab === 'service-graph'
            ? 'min-h-0 flex-1 overflow-hidden'
            : 'min-h-0 flex-1 overflow-auto'
        }
      >
        {tab === 'spans' && (
          <TraceSpanExplorer
            traceList={pagedTraceList}
            fieldTraces={traceList}
            loadedTraceCount={pagedTraceList.length}
            sort={traceSort}
            sortEnabled={queryMode === 'fields'}
            queryMode={queryMode}
            fields={traceFields}
            listState={listState}
            listError={traceListQuery.error}
            recovery={recovery}
            recoveryCopy={recoveryCopy}
            selectedId={selectedId}
            visibleFields={visibleTraceFields}
            fieldFilter={fieldFilter}
            fieldPanelCollapsed={fieldPanelCollapsed}
            onFieldFilterChange={setFieldFilter}
            onFieldPanelCollapsedChange={setFieldPanelCollapsed}
            onToggleField={toggleTraceField}
            onInsertField={insertFieldFilter}
            onSelectTrace={setSelectedId}
            pagination={tracePaginationModel}
            onSortChange={changeTraceSort}
          />
        )}

        {tab === 'traces' && (
          <TraceTableExplorer
            traceList={pagedTraceList}
            fieldTraces={traceList}
            loadedTraceCount={pagedTraceList.length}
            sort={traceSort}
            sortEnabled={queryMode === 'fields'}
            queryMode={queryMode}
            fields={traceFields}
            listState={listState}
            listError={traceListQuery.error}
            recovery={recovery}
            recoveryCopy={recoveryCopy}
            selectedTrace={selectedTrace}
            selectedId={selectedId}
            visibleFields={visibleTraceFields}
            fieldFilter={fieldFilter}
            fieldPanelCollapsed={fieldPanelCollapsed}
            onFieldFilterChange={setFieldFilter}
            onFieldPanelCollapsedChange={setFieldPanelCollapsed}
            onToggleField={toggleTraceField}
            onInsertField={insertFieldFilter}
            onSelectTrace={setSelectedId}
            onViewSpans={(id) => {
              setSelectedId(id);
              selectTab('spans');
            }}
            pagination={tracePaginationModel}
            onSortChange={changeTraceSort}
          />
        )}

        {tab === 'service-graph' && (
          <ServiceGraphPanel
            range={range}
            data={topologyQuery.data}
            isLoading={topologyQuery.isLoading}
            isError={topologyQuery.isError}
            error={topologyQuery.error}
            onServiceSelect={(serviceId) => {
              const next = `service_name = '${quoteTraceValue(serviceId)}'`;
              setQueryMode('fields');
              setQueryDraft(next);
              setQueryText(next);
              selectTab('spans');
            }}
          />
        )}

        {tab === 'service-catalog' && (
          <ServiceCatalog
            edges={allEdges}
            totalRps={totalRps}
            maxP95={maxP95}
            avgErrRate={avgErrRate}
            traces={traceList}
          />
        )}
      </div>
    </div>
  );
}

function TraceStatsPanel({
  edges,
  totalRps,
  maxP95,
  avgErrRate,
}: {
  edges: webApi.TopologyEdge[];
  totalRps: number;
  maxP95: number;
  avgErrRate: number;
}) {
  const { t } = useTranslation('traces');
  return (
    <div className="flex border-b border-bd-0">
      <div className="flex-1 p-3 px-4">
        <div className={`mb-2 ${uiLabelClass}`}>
          {t('explore.service_graph.latency_distribution')}
        </div>
        <LatencyDistribution edges={edges} />
      </div>
      <div className="w-px bg-bd-0" />
      <div className="flex items-center gap-4 px-6 py-4 font-sans text-xs">
        <Metric label="edges" value={`${edges.length}`} cls="text-tx-0" />
        <Metric label={t('explore.service_graph.kpis.total_rps')} value={`${totalRps.toFixed(0)}`} cls="text-green-soft" />
        <Metric label={t('explore.service_graph.kpis.max_p95')} value={`${maxP95.toFixed(0)} ms`} cls="text-blue-soft" />
        <Metric label={t('explore.service_graph.kpis.err_rate')} value={`${(avgErrRate * 100).toFixed(2)}%`} cls={avgErrRate > 0.01 ? 'text-red-soft' : avgErrRate > 0.001 ? 'text-yellow-soft' : 'text-tx-1'} />
      </div>
    </div>
  );
}

function TraceQueryPanel({
  tab,
  tabs,
  queryMode,
  queryDraft,
  queryText,
  sqlDraft,
  parsedQuery,
  completionItems,
  running,
  canRun,
  collapsed,
  sqlPlaceholder,
  onTabChange,
  onQueryModeChange,
  onQueryDraftChange,
  onSqlDraftChange,
  onCollapsedChange,
  onApplySearch,
  onRefresh,
}: {
  tab: TraceTab;
  tabs: Array<QueryToolbarTab<TraceTab>>;
  queryMode: TraceQueryMode;
  queryDraft: string;
  queryText: string;
  sqlDraft: string;
  parsedQuery: ParsedTraceStatement;
  completionItems: CodeCompletionItem[];
  running: boolean;
  canRun: boolean;
  collapsed: boolean;
  sqlPlaceholder: string;
  onTabChange: (tab: TraceTab) => void;
  onQueryModeChange: (mode: TraceQueryMode) => void;
  onQueryDraftChange: (value: string) => void;
  onSqlDraftChange: (value: string) => void;
  onCollapsedChange: (collapsed: boolean) => void;
  onApplySearch: () => void;
  onRefresh: () => void;
}) {
  const { t } = useTranslation('traces');
  const activeDraft = queryMode === 'sql' ? sqlDraft : queryDraft;
  const isQueryTab = tab === 'spans' || tab === 'traces';
  // SQL 检索函数（MATCH / MATCH_TEXT）由后端能力驱动，仅 SQL 模式注入（fields 走 q 全文搜索）。
  const sqlFunctions = useSqlFunctionCompletions();
  return (
    <QueryWorkbench
      className="shrink-0"
      {...(!isQueryTab ? { bodyClassName: 'hidden' } : {})}
      toolbar={
        <>
          <QueryToolbarTabs tabs={tabs} activeId={tab} onChange={onTabChange} tone="indigo" />
          {isQueryTab && (
            <>
              <QueryToolbarGroup aria-label={t('explore.query.mode_aria')}>
                {TRACE_QUERY_MODES.map((item) => (
                  <QueryToolbarButton
                    key={item.id}
                    active={queryMode === item.id}
                    tone="indigo"
                    onClick={() => onQueryModeChange(item.id)}
                  >
                    {t(item.labelKey)}
                  </QueryToolbarButton>
                ))}
              </QueryToolbarGroup>
              <QuerySyntaxHelp mode={queryMode} scope="traces" compact />
            </>
          )}
          <div className="ml-auto flex flex-wrap items-center justify-end gap-1.5">
            <TimeRangeChip />
            {isQueryTab && (
              <ChromeButton
                variant="primary"
                onClick={onApplySearch}
                disabled={running || !canRun}
              >
                <Play className="h-3 w-3" aria-hidden="true" />
                {running ? t('explore.query.running') : t('explore.query.run')}
              </ChromeButton>
            )}
            <ChromeButton onClick={onRefresh} disabled={running}>
              <RefreshCw className="h-3 w-3" /> {t('explore.toolbar.refresh')}
            </ChromeButton>
          </div>
        </>
      }
    >
      {isQueryTab && (
        <>
          <QueryEditorFrame
            queryRef="A"
            value={activeDraft}
            onChange={queryMode === 'sql' ? onSqlDraftChange : onQueryDraftChange}
            onClear={() => {
              if (queryMode === 'sql') onSqlDraftChange('');
              else onQueryDraftChange('');
            }}
            clearLabel={t('explore.query.clear_query')}
            onModEnter={() => {
              if (!running && canRun) onApplySearch();
            }}
            language={queryMode === 'sql' ? 'sql' : 'field-query'}
            ariaLabel={queryMode === 'sql' ? 'Trace SQL query editor' : 'Trace field query editor'}
            placeholder={queryMode === 'sql' ? sqlPlaceholder : 'trace_id = "..." / service_name contains "checkout"'}
            collapsed={collapsed}
            onCollapsedChange={onCollapsedChange}
            collapseLabel={t('explore.query.collapse_editor')}
            expandLabel={t('explore.query.expand_editor')}
            summary={activeDraft || t('explore.query.empty_summary')}
            completionItems={queryMode === 'fields' ? completionItems : sqlFunctions}
            minHeight={160}
            maxHeight={320}
            lineNumbers
            resizable
          />
          {!collapsed && queryMode === 'fields' && (queryText || parsedQuery.filters.length > 0 || parsedQuery.rejected.length > 0) && (
            <div className="mt-2 flex flex-wrap gap-2 font-sans text-xs">
              {parsedQuery.q && (
                <span className="inline-flex h-6 items-center rounded-md border border-bd-0 bg-bg-2 px-2 text-tx-1">
                  {t('explore.query.free_text_label')} {parsedQuery.q}
                </span>
              )}
              {parsedQuery.filters.map((filter, index) => (
                <span
                  key={`${filter.field}-${filter.op}-${filter.value}-${index}`}
                  className="inline-flex h-6 items-center rounded-md border border-bd-0 bg-bg-2 px-2 text-tx-1"
                >
                  {filter.field} {filter.op} {filter.value}
                </span>
              ))}
              {parsedQuery.rejected.map((item) => (
                <span key={item} className="inline-flex h-6 items-center rounded-md border border-yellow/40 bg-yellow-dim px-2 text-yellow-soft">
                  ignored: {item}
                </span>
              ))}
            </div>
          )}
        </>
      )}
    </QueryWorkbench>
  );
}

function TraceListSortSelect({
  value,
  onChange,
}: {
  value: webApi.TraceListSort;
  onChange: (value: webApi.TraceListSort) => void;
}) {
  const { t } = useTranslation('traces');
  return (
    <Select
      value={value}
      onValueChange={(nextValue) => onChange(parseTraceListSort(nextValue))}
    >
      <SelectTrigger
        aria-label={t('explore.sort.aria')}
        className="h-7 w-fit min-w-0 border-0 bg-transparent px-1.5 py-0 text-xs font-semibold text-tx-2 shadow-none hover:bg-transparent hover:text-tx-0 data-[state=open]:border-0 data-[state=open]:bg-transparent data-[state=open]:text-tx-0"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent align="end" className="min-w-[136px]">
        {TRACE_LIST_SORT_OPTIONS.map((option) => (
          <SelectItem key={option.value} value={option.value} className="h-8 text-xs">
            {t(option.labelKey)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function TraceSpanExplorer({
  traceList,
  fieldTraces,
  loadedTraceCount,
  sort,
  sortEnabled,
  queryMode,
  fields,
  listState,
  listError,
  recovery,
  recoveryCopy,
  selectedId,
  visibleFields,
  fieldFilter,
  fieldPanelCollapsed,
  onFieldFilterChange,
  onFieldPanelCollapsedChange,
  onToggleField,
  onInsertField,
  onSelectTrace,
  pagination,
  onSortChange,
}: {
  traceList: DisplayTrace[];
  fieldTraces: DisplayTrace[];
  loadedTraceCount: number;
  sort: webApi.TraceListSort;
  sortEnabled: boolean;
  queryMode: TraceQueryMode;
  fields: TraceFieldDef[];
  listState: ReturnType<typeof queryStateFor>;
  listError: unknown;
  recovery: QueryRecoveryActions;
  recoveryCopy: QueryRecoveryCopy;
  selectedId: string | null;
  visibleFields: TraceFieldName[];
  fieldFilter: string;
  fieldPanelCollapsed: boolean;
  onFieldFilterChange: (value: string) => void;
  onFieldPanelCollapsedChange: (collapsed: boolean) => void;
  onToggleField: (field: TraceFieldName) => void;
  onInsertField: (field: TraceFieldDef) => void;
  onSelectTrace: (id: string) => void;
  pagination: TracePaginationModel;
  onSortChange: (sort: webApi.TraceListSort) => void;
}) {
  const { t } = useTranslation('traces');
  const fmt = useTimeFormatter();
  return (
    <div className="flex h-full min-h-0 w-full items-stretch">
      <TraceFieldPanel
        traces={fieldTraces}
        fields={fields}
        queryMode={queryMode}
        visibleFields={visibleFields}
        fieldFilter={fieldFilter}
        collapsed={fieldPanelCollapsed}
        onFieldFilterChange={onFieldFilterChange}
        onCollapsedChange={onFieldPanelCollapsedChange}
        onToggleField={onToggleField}
        onInsertField={onInsertField}
      />
      <section
        data-workspace-pane="trace-results"
        className="flex min-h-0 w-[340px] shrink-0 flex-col overflow-hidden border-r border-bd-0 bg-bg-0 [contain:size]"
      >
        <div className="flex h-11 shrink-0 items-center justify-between gap-3 border-b border-bd-0 px-3 font-sans text-xs">
          <span className="min-w-0 truncate text-tx-1">
            {t('explore.results.loaded_count', { count: loadedTraceCount })}
          </span>
          {sortEnabled ? (
            <TraceListSortSelect value={sort} onChange={onSortChange} />
          ) : null}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {listState ? (
            <QueryRecoveryState
              state={listState}
              error={listError}
              copy={recoveryCopy}
              recovery={recovery}
              className="h-full min-h-0"
              testId="traces-no-data"
            />
          ) : (
            traceList.map((trace) => {
              const selected = selectedId === trace.id;
              return (
                <button
                  key={trace.id}
                  type="button"
                  title={trace.id}
                  aria-label={t('explore.results.select_trace_aria', { operation: trace.op })}
                  onClick={() => onSelectTrace(trace.id)}
                  className={`block min-h-16 w-full border-b border-l-2 border-bd-0 px-3 py-2.5 text-left font-sans text-xs transition-colors hover:bg-bg-2 focus-visible:bg-bg-2 ${
                    selected
                      ? 'border-l-indigo bg-indigo-dim'
                      : 'border-l-transparent bg-bg-0'
                  }`}
                >
                  <span className="grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-3">
                    <TraceOperationName operation={trace.op} className="font-semibold text-tx-0" />
                    <span className="whitespace-nowrap font-mono font-bold tabular-nums text-tx-0">
                      {formatTraceDurationMs(trace.durationMs)}
                    </span>
                  </span>
                  <span className="mt-1.5 grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3">
                    <span className="flex min-w-0 items-center gap-1.5 overflow-hidden text-tx-2">
                      <span className="truncate">{trace.service}</span>
                      <span className="shrink-0 text-tx-4">·</span>
                      <span className="shrink-0">
                        {t('explore.results.span_count', { count: trace.spans })}
                      </span>
                      {trace.errors > 0 ? (
                        <>
                          <span className="shrink-0 text-tx-4">·</span>
                          <span className="shrink-0 font-semibold text-red-soft">
                            {t('explore.results.error_count', { count: trace.errors })}
                          </span>
                        </>
                      ) : null}
                    </span>
                    <time className="whitespace-nowrap font-mono text-tx-3">
                      {formatTraceStart(trace.startNs, fmt.tz)}
                    </time>
                  </span>
                </button>
              );
            })
          )}
        </div>
        <TracePagination model={pagination} />
      </section>

      <section
        data-workspace-pane="trace-detail"
        className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-bg-0 [contain:size]"
      >
        <div className="flex h-11 shrink-0 items-center gap-2 border-b border-bd-0 px-3 font-sans text-xs text-tx-2">
          <span className="min-w-0 flex-1 truncate">
            {selectedId ? selectedId : t('detail.title')}
          </span>
          {selectedId && (
            <Link
              to={`/traces/${encodeURIComponent(selectedId)}`}
              className="shrink-0 whitespace-nowrap text-blue-soft hover:underline"
            >
              {t('explore.results.open_detail')}
            </Link>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto p-4">
          {selectedId ? (
            <TraceFlame traceId={selectedId} />
          ) : (
            <EmptyState
              strategy="query-first"
              title={t('explore.results.select_prompt')}
              description={t('explore.results.select_description')}
              className="min-h-0"
            />
          )}
        </div>
      </section>
    </div>
  );
}

function TraceTableExplorer({
  traceList,
  fieldTraces,
  loadedTraceCount,
  sort,
  sortEnabled,
  queryMode,
  fields,
  listState,
  listError,
  recovery,
  recoveryCopy,
  selectedTrace,
  selectedId,
  visibleFields,
  fieldFilter,
  fieldPanelCollapsed,
  onFieldFilterChange,
  onFieldPanelCollapsedChange,
  onToggleField,
  onInsertField,
  onSelectTrace,
  onViewSpans,
  pagination,
  onSortChange,
}: {
  traceList: DisplayTrace[];
  fieldTraces: DisplayTrace[];
  loadedTraceCount: number;
  sort: webApi.TraceListSort;
  sortEnabled: boolean;
  queryMode: TraceQueryMode;
  fields: TraceFieldDef[];
  listState: ReturnType<typeof queryStateFor>;
  listError: unknown;
  recovery: QueryRecoveryActions;
  recoveryCopy: QueryRecoveryCopy;
  selectedTrace: DisplayTrace | null;
  selectedId: string | null;
  visibleFields: TraceFieldName[];
  fieldFilter: string;
  fieldPanelCollapsed: boolean;
  onFieldFilterChange: (value: string) => void;
  onFieldPanelCollapsedChange: (collapsed: boolean) => void;
  onToggleField: (field: TraceFieldName) => void;
  onInsertField: (field: TraceFieldDef) => void;
  onSelectTrace: (id: string) => void;
  onViewSpans: (id: string) => void;
  pagination: TracePaginationModel;
  onSortChange: (sort: webApi.TraceListSort) => void;
}) {
  const { t } = useTranslation('traces');
  const [tzOverride, setTzOverride] = React.useState('');
  const [summaryOpen, setSummaryOpen] = React.useState(false);
  const fmt = useTimeFormatter({ timezone: tzOverride || undefined });

  const selectTrace = React.useCallback((id: string) => {
    onSelectTrace(id);
    setSummaryOpen(true);
  }, [onSelectTrace]);

  return (
    <div className="flex h-full min-h-0 w-full items-stretch">
      <TraceFieldPanel
        traces={fieldTraces}
        fields={fields}
        queryMode={queryMode}
        visibleFields={visibleFields}
        fieldFilter={fieldFilter}
        collapsed={fieldPanelCollapsed}
        onFieldFilterChange={onFieldFilterChange}
        onCollapsedChange={onFieldPanelCollapsedChange}
        onToggleField={onToggleField}
        onInsertField={onInsertField}
      />
      <div
        data-workspace-pane="trace-results"
        className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden border-r border-bd-0 bg-bg-0 [contain:size]"
      >
        <div className="flex min-h-11 shrink-0 items-center justify-between gap-3 border-b border-bd-0 bg-bg-0 px-3 py-1.5">
          <span className="font-sans text-xs font-semibold text-tx-0">
            {t('explore.results.loaded_count', { count: loadedTraceCount })}
          </span>
          <div className="flex shrink-0 items-center gap-1.5">
            {sortEnabled ? (
              <TraceListSortSelect value={sort} onChange={onSortChange} />
            ) : null}
            <TimezoneSelect value={tzOverride} onChange={setTzOverride} className="h-8" />
          </div>
        </div>
        <div className="grid shrink-0 grid-cols-[minmax(180px,1.4fr)_160px_minmax(180px,1fr)_72px_72px_96px_150px] gap-3 border-b border-bd-0 bg-bg-1 px-3 py-2 font-sans text-xs font-strong uppercase tracking-normal text-tx-2">
          <span>{t('explore.table.trace')}</span>
          <span>{t('explore.table.service')}</span>
          <span>{t('explore.table.operation')}</span>
          <span>{t('explore.table.spans')}</span>
          <span>{t('explore.table.errors')}</span>
          <span>{t('explore.table.duration')}</span>
          <span>{t('explore.table.start')}</span>
        </div>
        <div className="min-h-0 flex-1 overflow-auto">
          {listState ? (
            <QueryRecoveryState
              state={listState}
              error={listError}
              copy={recoveryCopy}
              recovery={recovery}
              className="h-full min-h-0"
              testId="traces-table-no-data"
            />
          ) : (
            traceList.map((trace) => (
              <button
                key={trace.id}
                type="button"
                onClick={() => selectTrace(trace.id)}
                className={`grid w-full grid-cols-[minmax(180px,1.4fr)_160px_minmax(180px,1fr)_72px_72px_96px_150px] gap-3 border-b border-bd-0 px-3 py-2 text-left font-sans text-xs hover:bg-bg-2 ${
                  selectedId === trace.id ? 'border-l-2 border-l-orange bg-bg-2' : 'border-l-2 border-l-transparent'
                }`}
              >
                <span className="min-w-0">
                  <span className="block truncate font-semibold text-tx-0">{trace.id}</span>
                  <span className="mt-1 block truncate text-xs text-tx-3">{t('explore.results.inspect_hint')}</span>
                </span>
                <span className="truncate text-tx-1">{trace.service}</span>
                <TraceOperationName operation={trace.op} className="text-tx-1" />
                <span className="text-tx-1">{trace.spans}</span>
                <span className={trace.errors > 0 ? 'text-red' : 'text-tx-2'}>{trace.errors}</span>
                <span className="font-semibold text-tx-0">{formatTraceDurationMs(trace.durationMs)}</span>
                <span className="truncate text-tx-2">{formatTraceStart(trace.startNs, fmt.tz)}</span>
              </button>
            ))
          )}
        </div>
        <TracePagination model={pagination} />
      </div>
      {summaryOpen && selectedTrace && (
        <>
          <button
            type="button"
            aria-label={t('explore.results.close_summary')}
            tabIndex={-1}
            onClick={() => setSummaryOpen(false)}
            className="fixed bottom-0 left-0 right-0 top-topbar z-[55] cursor-default border-0 bg-transparent p-0 focus:outline-none"
          />
          <aside
            aria-label={t('explore.results.summary_drawer_aria')}
            className="fixed bottom-0 right-0 top-topbar z-[60] min-h-0 w-[34vw] min-w-[420px] max-w-[660px] border-l border-bd-1 bg-bg-0 shadow-drawer data-[state=open]:animate-slide-in-right"
            data-state="open"
          >
            <TraceSummaryPanel
              trace={selectedTrace}
              visibleFields={visibleFields}
              onViewSpans={onViewSpans}
              onClose={() => setSummaryOpen(false)}
            />
          </aside>
        </>
      )}
    </div>
  );
}

function TraceSummaryPanel({
  trace,
  visibleFields,
  onViewSpans,
  onClose,
}: {
  trace: DisplayTrace;
  visibleFields: TraceFieldName[];
  onViewSpans: (id: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation('traces');

  return (
    <div className="flex h-full min-h-0 flex-col bg-bg-0">
      <div className="flex items-start gap-3 border-b border-bd-0 px-4 py-3">
        <div className="min-w-0 flex-1">
          <TraceOperationName
            operation={trace.op}
            className="font-sans text-sm font-bold text-tx-0"
          />
          <div className="mt-1 truncate font-sans text-xs text-tx-2">
            {/* Phase 6 M2: trace ID is the canonical cross-signal handle. */}
            <SignalReference type="trace_id" value={trace.id} labels={{ trace_id: trace.id, service: trace.service }}>
              {trace.id}
            </SignalReference>
          </div>
        </div>
        <Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" onClick={onClose} aria-label={t('explore.results.close_summary')}>
          <X className="h-4 w-4" />
        </Button>
      </div>
      <div className="grid grid-cols-2 border-b border-bd-0">
        <TraceSummaryMetric
          label="service"
          value={
            <SignalReference type="service" value={trace.service} labels={{ trace_id: trace.id, service: trace.service }}>
              {trace.service}
            </SignalReference>
          }
        />
        <TraceSummaryMetric label="duration" value={formatTraceDurationMs(trace.durationMs)} />
        <TraceSummaryMetric label="spans" value={String(trace.spans)} />
        <TraceSummaryMetric label="errors" value={String(trace.errors)} {...(trace.errors > 0 ? { tone: 'text-red' } : {})} />
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-4">
        <div className="mb-2 font-sans text-xs font-strong uppercase tracking-normal text-tx-2">{t('explore.results.visible_labels')}</div>
        <div className="flex flex-wrap gap-1.5">
          {visibleFields.map((field) => {
            const value = traceFieldValue(trace, field);
            if (!value) return null;
            const signalType = traceFieldSignalType(field);
            return (
              <span key={field} className="inline-flex max-w-full items-center gap-1 rounded border border-bd-0 bg-bg-1 px-2 py-1 font-sans text-xs">
                <span className="text-tx-3">{field}</span>
                {signalType ? (
                  <SignalReference
                    type={signalType}
                    value={value}
                    labelName={field}
                    labels={{ trace_id: trace.id, service: trace.service, [field]: value }}
                    className="max-w-[230px] truncate"
                  >
                    {value}
                  </SignalReference>
                ) : (
                  <span className="max-w-[230px] truncate text-tx-1">{value}</span>
                )}
              </span>
            );
          })}
        </div>
      </div>
      <div className="border-t border-bd-0 p-3">
        <Button className="w-full" onClick={() => onViewSpans(trace.id)}>
          {t('explore.results.view_spans')}
        </Button>
      </div>
    </div>
  );
}

function TraceSummaryMetric({ label, value, tone }: { label: string; value: React.ReactNode; tone?: string }) {
  return (
    <div className="min-w-0 border-b border-r border-bd-0 px-4 py-3 even:border-r-0">
      <div className="font-sans text-xs font-semibold uppercase tracking-normal text-tx-3">{label}</div>
      <div className={`mt-1 truncate font-sans text-sm font-semibold ${tone ?? 'text-tx-0'}`}>{value}</div>
    </div>
  );
}

/**
 * Map trace field names → SignalReference signal types so the visible-
 * labels chips automatically wrap cross-signal handles in a HoverCard.
 * Returning `null` falls back to plain text.
 */
function traceFieldSignalType(field: TraceFieldName): SignalReferenceType | null {
  switch (field) {
    case 'trace_id':
      return 'trace_id';
    case 'span_id':
    case 'parent_span_id':
      return 'span_id';
    case 'service.name':
      return 'service';
    default:
      return null;
  }
}

function formatTraceStart(startNs: number, tz: string): string {
  if (!Number.isFinite(startNs) || startNs <= 0) return '-';
  const d = dayjs(Math.floor(startNs / 1_000_000)).tz(tz);
  return d.isValid() ? d.format('HH:mm:ss.SSS') : '-';
}

function ServiceCatalog({
  edges,
  totalRps,
  maxP95,
  avgErrRate,
  traces,
}: {
  edges: webApi.TopologyEdge[];
  totalRps: number;
  maxP95: number;
  avgErrRate: number;
  traces: DisplayTrace[];
}) {
  const { t } = useTranslation('traces');
  const byService = traces.reduce<Map<string, { count: number; errors: number; maxDuration: number }>>((acc, trace) => {
    const current = acc.get(trace.service) ?? { count: 0, errors: 0, maxDuration: 0 };
    current.count += 1;
    current.errors += trace.errors;
    current.maxDuration = Math.max(current.maxDuration, trace.durationMs);
    acc.set(trace.service, current);
    return acc;
  }, new Map());
  return (
    <>
      <TraceStatsPanel edges={edges} totalRps={totalRps} maxP95={maxP95} avgErrRate={avgErrRate} />
      <div className="grid gap-3 p-3 xl:grid-cols-2">
        <Card>
          <CardHeader title={t('explore.service_catalog.title')} />
          <CardBody className="p-0">
            {[...byService.entries()].map(([service, item]) => (
              <div key={service} className="grid grid-cols-[minmax(0,1fr)_80px_80px_96px] gap-3 border-b border-bd-0 px-3 py-2 font-sans text-xs last:border-b-0">
                <span className="truncate font-semibold text-tx-0">{service}</span>
                <span className="text-tx-2">{item.count} traces</span>
                <span className={item.errors > 0 ? 'text-red' : 'text-tx-2'}>{item.errors} errors</span>
                <span className="text-right font-semibold text-tx-0">{item.maxDuration.toFixed(1)} ms</span>
              </div>
            ))}
            {byService.size === 0 && (
              <div className="p-4 font-sans text-xs text-tx-3">{t('explore.service_catalog.empty')}</div>
            )}
          </CardBody>
        </Card>
        <Card>
          <CardHeader title={t('explore.service_catalog.slowest_operations')} />
          <CardBody className="p-0">
            {[...traces].sort((a, b) => b.durationMs - a.durationMs).slice(0, 8).map((trace) => (
              <button
                key={trace.id}
                type="button"
                className="grid w-full grid-cols-[minmax(0,1fr)_96px] gap-3 border-b border-bd-0 px-3 py-2 text-left font-sans text-xs hover:bg-bg-2 last:border-b-0"
              >
                <span className="min-w-0">
                  <TraceOperationName
                    operation={trace.op}
                    className="font-semibold text-tx-0"
                  />
                  <span className="block truncate text-xs text-tx-3">{trace.service}</span>
                </span>
                <span className="text-right font-semibold text-tx-0">{formatTraceDurationMs(trace.durationMs)}</span>
              </button>
            ))}
          </CardBody>
        </Card>
      </div>
    </>
  );
}

function Metric({ label, value, cls }: { label: string; value: string; cls: string }) {
  return (
    <div>
      <div className={uiLabelClass}>{label}</div>
      <div className={`mt-0.5 text-base ${cls}`}>{value}</div>
    </div>
  );
}

function LatencyDistribution({ edges }: { edges: webApi.TopologyEdge[] }) {
  const { t } = useTranslation('traces');
  const buckets = [
    { label: t('explore.service_graph.latency_buckets.under_50ms'), max: 50 },
    { label: '50-100', max: 100 },
    { label: '100-250', max: 250 },
    { label: '250-500', max: 500 },
    { label: t('explore.service_graph.latency_buckets.over_500ms'), max: Number.POSITIVE_INFINITY },
  ].map((bucket, index, arr) => {
    const min = index === 0 ? 0 : (arr[index - 1]?.max ?? 0);
    const count = edges.filter((edge) => edge.p95_ms >= min && edge.p95_ms < bucket.max).length;
    return { ...bucket, count };
  });
  const maxCount = Math.max(...buckets.map((bucket) => bucket.count), 1);
  const slowest = [...edges].sort((a, b) => b.p95_ms - a.p95_ms).slice(0, 3);

  if (edges.length === 0) {
    return (
      <div className="grid h-[112px] place-items-center rounded-md border border-dashed border-bd-1 bg-bg-0 font-sans text-xs text-tx-3">
        {t('explore.service_graph.no_latency')}
      </div>
    );
  }

  return (
    <div className="grid min-h-[112px] gap-4 lg:grid-cols-[minmax(0,1fr)_240px]">
      <div className="relative flex min-w-0 items-end gap-2 overflow-hidden rounded-md border border-bd-0 bg-bg-0 px-3 pb-7 pt-3">
        <div aria-hidden className="pointer-events-none absolute inset-x-3 bottom-7 border-t border-bd-1" />
        {buckets.map((bucket) => (
          <div key={bucket.label} className="relative flex min-w-0 flex-1 flex-col items-center justify-end">
            <div className="mb-1 font-sans text-xs font-semibold text-tx-2">{bucket.count}</div>
            <Tooltip>
              <TooltipTrigger asChild>
                <div
                  role="img"
                  aria-label={t('explore.service_graph.bucket_aria', {
                    label: bucket.label,
                    count: bucket.count,
                  })}
                  className={`w-full cursor-crosshair rounded-t-sm transition-[filter] hover:brightness-110 ${
                    bucket.count === 0 ? 'bg-bd-1' : 'bg-blue-soft'
                  }`}
                  style={{ height: `${bucket.count === 0 ? 3 : Math.max(12, (bucket.count / maxCount) * 56)}px` }}
                />
              </TooltipTrigger>
              <TooltipPortal>
                <TooltipContent side="top" sideOffset={8} className="px-3 py-2">
                  <div className="font-sans text-xs font-semibold text-tx-0">{bucket.label}</div>
                  <div className="mt-1 font-sans text-xs text-tx-2">
                    {t('explore.service_graph.edge_count', { count: bucket.count })}
                  </div>
                </TooltipContent>
              </TooltipPortal>
            </Tooltip>
            <div className="absolute -bottom-5 w-full truncate text-center font-sans text-xs text-tx-3">
              {bucket.label}
            </div>
          </div>
        ))}
      </div>
      <div className="rounded-md border border-bd-0 bg-bg-0 p-3">
        <div className="mb-2 font-sans text-xs font-semibold uppercase tracking-normal text-tx-2">
          {t('explore.service_graph.slowest_edges')}
        </div>
        <div className="space-y-1.5">
          {slowest.map((edge) => (
            <div key={`${edge.source}-${edge.target}`} className="grid grid-cols-[minmax(0,1fr)_auto] gap-2 font-sans text-xs">
              <span className="truncate text-tx-1">
                {edge.source}
                {' -> '}
                {edge.target}
              </span>
              <span className="font-semibold text-tx-0">{edge.p95_ms.toFixed(0)} ms</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
