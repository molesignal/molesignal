import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as queryApi from '@/api/query';
import * as streamsApi from '@/api/streams';
import * as webApi from '@/api/web';
import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import type { CursorPage } from '@/pagination/cursor';
import { explorerPageSizeIn, explorerPageSizesFor } from '@/pagination/explorerPageSize';
import { useCursorPagination } from '@/pagination/useCursorPagination';
import {
  Card,
  CardBody,
  CardHeader,
  uiLabelClass,
} from '@/shell/chrome';
import type { QueryEditorMode } from '@/shell/query/ModeSwitch';
import {
  type QueryRecoveryActions,
  type QueryRecoveryCopy,
} from '@/shell/query/RecoveryState';
import { queryStateFor } from '@/shell/query/State';
import { useFoldedWhileNarrow, useQueryTabSearchParams, useQueryTabStatus } from '@/shell/queryTabs';
import { Tooltip, TooltipContent, TooltipPortal, TooltipTrigger } from '@/shell/ui/tooltip';
import { isNarrowViewport } from '@/shell/useViewportWidth';
import { useAuthStore } from '@/stores/auth';
import { resolveWindow, type TimeWindow } from '@/stores/useTimeStore';
import { formatTraceDurationMs } from '@/viz/trace/duration';
import { AttributeFilterContext, appendAttributeClause, attributeClause } from '@/viz/trace/span-detail/model';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { traceQueryCompletions } from './explorer/completions';
import { useTraceFieldFilters } from './explorer/fieldFilters';
import { useTraceHistogram } from './explorer/Histogram';
import { useTraceQueryDraft } from './explorer/queryDraft';
import { TraceQueryPanel } from './explorer/QueryPanel';
import { TraceSpanExplorer } from './explorer/SpanWorkspace';
import { traceSqlResultToDisplayTraces } from './explorer/sqlResult';
import { TraceTableExplorer } from './explorer/TableWorkspace';
import { useTraceTimeWindow } from './explorer/timeWindow';
import type { DisplayTrace, TraceTab } from './explorer/types';
import { TraceFieldPanel } from './fieldPanel/Panel';
import {
  DEFAULT_VISIBLE_TRACE_FIELDS,
  deriveTraceFields,
  parseTraceStatement,
  selectTraceStream,
  TRACE_RESULT_LIMIT,
  traceSqlPlaceholder,
  type TraceFieldName,
  type TraceQueryMode,
} from './fieldQueryModel';
import {
  DEFAULT_TRACE_PAGE_SIZE,
  type TracePaginationModel,
} from './Pagination';
import { ServiceGraphPanel } from './serviceGraph/Panel';
import {
  parseTraceListSort,
  writeTraceListSort,
} from './sort';
import {
  quoteTraceValue,
  traceUrlQueryStateFromParams,
  traceUrlQueryStateKey,
} from './urlState';

type TraceListData = CursorPage<DisplayTrace>;

const TRACE_TABS: Array<{ id: TraceTab; labelKey: string }> = [
  { id: 'spans', labelKey: 'explore.tabs.spans' },
  { id: 'traces', labelKey: 'explore.tabs.traces' },
  { id: 'service-graph', labelKey: 'explore.tabs.service_graph' },
  { id: 'service-catalog', labelKey: 'explore.tabs.service_catalog' },
];

function rangeFromWindow(window: TimeWindow, now: Date): { from: string; to: string } {
  const resolvedWindow = resolveWindow(window, now);
  return { from: resolvedWindow.from.toISOString(), to: resolvedWindow.to.toISOString() };
}

function tabFromParam(value: string | null): TraceTab | null {
  if (value === 'map') return 'service-graph';
  if (value === 'analytics') return 'service-catalog';
  return TRACE_TABS.some((tab) => tab.id === value) ? (value as TraceTab) : null;
}

export function Traces() {
  const { t, i18n } = useTranslation('traces');
  const orgId = useAuthStore((s) => s.ctx?.org_id ?? '');
  // The page can be one of several query tabs: its params and its time range
  // are the tab's own.
  const [searchParams, setSearchParams] = useQueryTabSearchParams();
  const { timeWindow, effectiveTimeWindow, setTimeWindow } = useTraceTimeWindow(searchParams);
  const traceSort = parseTraceListSort(searchParams.get('sort'));
  const requestedTraceStream = searchParams.get('stream')?.trim() ?? '';
  const urlTabParam = searchParams.get('tab') ?? searchParams.get('view');
  const urlTab = tabFromParam(urlTabParam);
  const initialUrlQuery = React.useRef(
    traceUrlQueryStateFromParams(searchParams),
  ).current;
  const [tab, setTab] = React.useState<TraceTab>(() => urlTab ?? 'spans');
  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const {
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
  } = useTraceQueryDraft(initialUrlQuery);
  const [queryText, setQueryText] = React.useState(initialUrlQuery.fields);
  const [sqlText, setSqlText] = React.useState(initialUrlQuery.sql);
  const [fieldFilter, setFieldFilter] = React.useState('');
  // Folded in a narrow window, and for as long as the page has a narrow share of a split one.
  const [fieldPanelCollapsed, setFieldPanelCollapsed] = useFoldedWhileNarrow(isNarrowViewport);
  const [queryEditorCollapsed, setQueryEditorCollapsed] = React.useState(
    !initialUrlQuery.fields && !initialUrlQuery.sql,
  );
  const [visibleTraceFields, setVisibleTraceFields] = React.useState<TraceFieldName[]>(DEFAULT_VISIBLE_TRACE_FIELDS);
  const [tracePage, setTracePage] = React.useState(1);
  const [rangeRefreshAt, setRangeRefreshAt] = React.useState(() => Date.now());
  // Goes up when the reader asks for the results again: the histogram counts them again too.
  const [countEpoch, setCountEpoch] = React.useState(0);
  const appliedTraceQueryRef = React.useRef(
    traceUrlQueryStateKey(initialUrlQuery),
  );
  const appliedTabParamRef = React.useRef<string | null>(urlTabParam);

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
  }, [searchParams, setQueryDraft, setQueryMode, setSqlDraft]);

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
  // What a statement returned is all in hand and paged here: no size is offered beyond the one that shows it all.
  const statementPageSizes = React.useMemo(() => explorerPageSizesFor(traceList.length), [traceList.length]);
  const statementPageSize = explorerPageSizeIn(statementPageSizes, tracePageSize);
  const tracePageCount = queryMode === 'sql'
    ? Math.max(1, Math.ceil(traceList.length / statementPageSize))
    : 1;
  const activeTracePage = Math.min(tracePage, tracePageCount);
  const tracePageStart = (activeTracePage - 1) * statementPageSize;
  const pagedTraceList = React.useMemo(
    () => queryMode === 'fields'
      ? traceList
      : traceList.slice(tracePageStart, tracePageStart + statementPageSize),
    [queryMode, statementPageSize, traceList, tracePageStart],
  );
  // The histogram counts what the list query matches, so it is given the query the list sends.
  // A statement has no list query: its histogram is of the traces it returned.
  const countedQuery = React.useMemo(
    () =>
      queryMode === 'fields'
        ? { orgId, q: parsedQuery.q, filters: parsedQuery.filters, epoch: countEpoch }
        : null,
    [countEpoch, orgId, parsedQuery, queryMode],
  );
  const traceHistogram = useTraceHistogram({
    traces: traceList,
    range,
    onTimeWindowChange: setTimeWindow,
    counted: countedQuery,
  });

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
          pageSize: statementPageSize,
          pageSizes: statementPageSizes,
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
      statementPageSize,
      statementPageSizes,
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

  // What a link to this view carries of the page's own: the view, the order and the query that is applied.
  const shareSearch = React.useMemo(() => {
    const params = new URLSearchParams();
    for (const key of ['tab', 'view', 'stream', 'sort']) {
      const value = searchParams.get(key);
      if (value) params.set(key, value);
    }

    const statement = (queryMode === 'sql' ? sqlText : queryText).trim();
    if (statement) params.set(queryMode === 'sql' ? 'sql' : 'q', statement);

    return params;
  }, [queryMode, queryText, searchParams, sqlText]);

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

  const changeEditorMode = React.useCallback((nextMode: QueryEditorMode) => {
    setEditorMode(nextMode);
    setQueryEditorCollapsed(false);
  }, [setEditorMode]);

  const toggleTraceField = React.useCallback((field: TraceFieldName) => {
    setVisibleTraceFields((current) => {
      if (current.includes(field)) return current.filter((item) => item !== field);
      return [...current, field];
    });
  }, []);

  const openQueryEditor = React.useCallback(() => setQueryEditorCollapsed(false), []);
  const runStatement = React.useCallback((mode: TraceQueryMode, statement: string) => {
    if (mode === 'sql') setSqlText(statement.trim());
    else setQueryText(statement.trim());
  }, []);
  const { filterMode, insertField, filterValue, isValueFiltered } = useTraceFieldFilters({
    editorMode,
    queryMode,
    queryDraft,
    sqlDraft,
    stream: primaryTraceStream,
    setBuilderField,
    setQueryMode,
    setQueryDraft,
    setSqlDraft,
    onDraftStarted: openQueryEditor,
    onRun: runStatement,
  });
  const fieldValueSource = React.useMemo(
    () => ({
      orgId,
      stream: primaryTraceStream,
      range: { start: Date.parse(range.from) * 1000, end: Date.parse(range.to) * 1000 },
    }),
    [orgId, primaryTraceStream, range.from, range.to],
  );
  // One field list for both views. Only the Traces view has a place beside a picked trace
  // for the fields the eye chooses, so only there is the eye offered.
  const fieldPanel = (
    <TraceFieldPanel
      traces={traceList}
      fields={traceFields}
      source={fieldValueSource}
      filterMode={filterMode}
      fieldFilter={fieldFilter}
      collapsed={fieldPanelCollapsed}
      onFieldFilterChange={setFieldFilter}
      onCollapsedChange={setFieldPanelCollapsed}
      onInsertField={insertField}
      isValueFiltered={isValueFiltered}
      onFilterValue={filterValue}
      {...(tab === 'traces' ? { visibleFields: visibleTraceFields, onToggleField: toggleTraceField } : {})}
      className="rounded-md border-r-0 bg-[var(--functional-surface)]"
    />
  );

  const listState = queryStateFor({
    isLoading: traceListQuery.isLoading,
    isError: traceListQuery.isError,
    data: traceList,
  });
  const traceQueryRunning = traceListQuery.isFetching;
  useQueryTabStatus({
    running: traceQueryRunning,
    summary: (queryMode === 'sql' ? sqlText : queryText).trim().replace(/\s+/g, ' '),
  });
  const traceQueryCanRun = queryMode === 'fields'
    || Boolean(orgId && primaryTraceStream && sqlDraft.trim());

  // Aggregate stats from topology (real data!)
  const allEdges = topologyQuery.data?.edges ?? [];
  const totalRps = allEdges.reduce((a, e) => a + e.rps, 0);
  const avgErrRate = allEdges.length > 0 ? allEdges.reduce((a, e) => a + e.err_rate, 0) / allEdges.length : 0;
  const maxP95 = allEdges.reduce((a, e) => Math.max(a, e.p95_ms), 0);
  const refreshCurrentView = React.useCallback(() => {
    setCountEpoch((epoch) => epoch + 1);
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
  }, [resetTraceCursor, searchParams, setQueryDraft, setQueryMode, setSearchParams, setSqlDraft]);
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
    docsHref: `https://docs.molesignal.com/${
      (i18n.resolvedLanguage ?? i18n.language).toLowerCase().startsWith('zh') ? 'zh-Hans' : 'en-US'
    }/traces`,
  };

  return (
    <div
      data-workspace="traces"
      className="flex h-full min-h-0 flex-col overflow-hidden bg-[var(--page-canvas)]"
    >
      <TraceQueryPanel
        tab={tab}
        tabs={TRACE_TABS.map((tb) => ({
          id: tb.id,
          label: t(tb.labelKey),
          count: tb.id === 'spans' ? (selectedTrace?.spans ?? 0) : tb.id === 'traces' ? traceList.length : undefined,
        }))}
        editorMode={editorMode}
        queryMode={queryMode}
        traceFields={traceFields}
        builderField={builderField}
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
        onEditorModeChange={changeEditorMode}
        onCodeDraftChange={changeCodeDraft}
        onBuilderDraftChange={changeBuilderDraft}
        onBuilderFieldHandled={() => setBuilderField(null)}
        onCollapsedChange={setQueryEditorCollapsed}
        onApplySearch={applySearch}
        onRefresh={refreshCurrentView}
        shareSearch={shareSearch}
      />

      {/* Query workspaces fill the remaining viewport. Result panes own their
          scrolling so empty states stretch vertically and pagination stays
          pinned to the bottom edge. */}
      <div
        className={
          tab === 'spans' || tab === 'traces' || tab === 'service-graph'
            ? 'min-h-0 flex-1 overflow-hidden bg-[var(--page-canvas)] pl-[var(--pane-gutter-start,20px)] pr-[var(--pane-gutter-end,20px)]'
            : 'min-h-0 flex-1 overflow-auto bg-[var(--page-canvas)] pl-[var(--pane-gutter-start,20px)] pr-[var(--pane-gutter-end,20px)]'
        }
      >
        {tab === 'spans' && (
          <AttributeFilterContext.Provider value={queryMode === 'sql' ? null : (key, value, operator) => {
            setQueryDraft(current => appendAttributeClause(current, attributeClause(key, value, operator)));
            setQueryEditorCollapsed(false);
          }}>
          <TraceSpanExplorer
            fieldPanel={fieldPanel}
            traceList={pagedTraceList}
            loadedTraceCount={traceList.length}
            sort={traceSort}
            sortEnabled={queryMode === 'fields'}
            listState={listState}
            listError={traceListQuery.error}
            recovery={recovery}
            recoveryCopy={recoveryCopy}
            selectedId={selectedId}
            onSelectTrace={setSelectedId}
            pagination={tracePaginationModel}
            histogram={traceHistogram}
            onSortChange={changeTraceSort}
          />
          </AttributeFilterContext.Provider>
        )}

        {tab === 'traces' && (
          <TraceTableExplorer
            fieldPanel={fieldPanel}
            traceList={pagedTraceList}
            loadedTraceCount={traceList.length}
            sort={traceSort}
            sortEnabled={queryMode === 'fields'}
            listState={listState}
            listError={traceListQuery.error}
            recovery={recovery}
            recoveryCopy={recoveryCopy}
            selectedTrace={selectedTrace}
            selectedId={selectedId}
            visibleFields={visibleTraceFields}
            onSelectTrace={setSelectedId}
            onViewSpans={(id) => {
              setSelectedId(id);
              selectTab('spans');
            }}
            pagination={tracePaginationModel}
            histogram={traceHistogram}
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
        <Metric label={t('explore.service_graph.kpis.total_rps')} value={`${totalRps.toFixed(0)}`} cls="text-[var(--green-ink)]" />
        <Metric label={t('explore.service_graph.kpis.max_p95')} value={`${maxP95.toFixed(0)} ms`} cls="text-[var(--blue-ink)]" />
        <Metric label={t('explore.service_graph.kpis.err_rate')} value={`${(avgErrRate * 100).toFixed(2)}%`} cls={avgErrRate > 0.01 ? 'text-red-soft' : avgErrRate > 0.001 ? 'text-[var(--yellow-ink)]' : 'text-tx-1'} />
      </div>
    </div>
  );
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
        <div className="mb-2 font-sans text-xs font-semibold tracking-normal text-tx-2">
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
