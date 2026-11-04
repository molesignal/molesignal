import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as streamsApi from '@/api/streams';
import * as webApi from '@/api/web';
import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import type { QueryEditorMode } from '@/shell/query/ModeSwitch';
import { type QueryRecoveryActions, type QueryRecoveryCopy } from '@/shell/query/RecoveryState';
import { queryStateFor } from '@/shell/query/State';
import { useFoldedWhileNarrow, useQueryTabSearchParams, useQueryTabState, useQueryTabStatus } from '@/shell/queryTabs';
import { isNarrowViewport } from '@/shell/useViewportWidth';
import { useAuthStore } from '@/stores/auth';
import { resolveWindow, type TimeWindow } from '@/stores/useTimeStore';
import { AttributeFilterContext, appendAttributeClause, attributeClause } from '@/viz/trace/span-detail/model';

import { traceQueryCompletions } from './explorer/completions';
import { useTraceFieldFilters } from './explorer/fieldFilters';
import { useTraceHistogram } from './explorer/Histogram';
import { useTraceList } from './explorer/list/useTraceList';
import { useTraceQueryDraft } from './explorer/queryDraft';
import { TraceQueryPanel } from './explorer/QueryPanel';
import { useSpanHistogram } from './explorer/spans/histogram';
import { parseSpanListSort, writeSpanListSort } from './explorer/spans/model';
import { useSpanList } from './explorer/spans/useSpanList';
import { SpanListExplorer } from './explorer/spans/Workspace';
import { useTraceColumns } from './explorer/table/useTraceColumns';
import { TraceTableExplorer } from './explorer/table/Workspace';
import { useTraceTimeWindow } from './explorer/timeWindow';
import type { TraceTab } from './explorer/types';
import { TraceFieldPanel } from './fieldPanel/Panel';
import { deriveTraceFields, parseTraceStatement, selectTraceStream, traceSqlPlaceholder, type TraceQueryMode } from './fieldQueryModel';
import { TraceRedView } from './red/View';
import { ServiceGraphPanel } from './serviceGraph/Panel';
import { parseTraceListSort, writeTraceListSort } from './sort';
import { quoteTraceValue, traceUrlQueryStateFromParams, traceUrlQueryStateKey, writeTraceQueryParams } from './urlState';

const TRACE_TABS: Array<{ id: TraceTab; labelKey: string }> = [
  { id: 'traces', labelKey: 'explore.tabs.traces' },
  { id: 'spans', labelKey: 'explore.tabs.spans' },
  { id: 'red', labelKey: 'explore.tabs.red' },
  { id: 'service-graph', labelKey: 'explore.tabs.service_graph' },
];
// The tab /traces opens on, and the one an address without `tab` names: the first of the list.
const DEFAULT_TRACE_TAB: TraceTab = 'traces';

function rangeFromWindow(window: TimeWindow, now: Date): { from: string; to: string } {
  const resolvedWindow = resolveWindow(window, now);
  return { from: resolvedWindow.from.toISOString(), to: resolvedWindow.to.toISOString() };
}

function tabFromParam(value: string | null): TraceTab | null {
  if (value === 'map') return 'service-graph';
  return TRACE_TABS.some((tab) => tab.id === value) ? (value as TraceTab) : null;
}

export function Traces() {
  const { t, i18n } = useTranslation('traces');
  const orgId = useAuthStore((s) => s.ctx?.orgId ?? '');
  // The page can be one of several query tabs: its params and its time range are the tab's own.
  const [searchParams, setSearchParams] = useQueryTabSearchParams();
  const { timeWindow, effectiveTimeWindow, setTimeWindow } = useTraceTimeWindow(searchParams);
  const traceSort = parseTraceListSort(searchParams.get('sort'));
  const spanSort = parseSpanListSort(searchParams.get('spanSort'));
  const requestedTraceStream = searchParams.get('stream')?.trim() ?? '';
  const urlTabParam = searchParams.get('tab') ?? searchParams.get('view');
  const urlTab = tabFromParam(urlTabParam);
  const initialUrlQuery = React.useRef(traceUrlQueryStateFromParams(searchParams)).current;
  const [tab, setTab] = React.useState<TraceTab>(() => urlTab ?? DEFAULT_TRACE_TAB);
  const draft = useTraceQueryDraft(initialUrlQuery);
  const { editorMode, setEditorMode, setBuilderField, queryMode, setQueryMode, queryDraft, setQueryDraft, sqlDraft, setSqlDraft } = draft;
  const [queryText, setQueryText] = React.useState(initialUrlQuery.fields);
  const [sqlText, setSqlText] = React.useState(initialUrlQuery.sql);
  const [fieldFilter, setFieldFilter] = React.useState('');
  // Folded in a narrow window, and for as long as the page has a narrow share of a split one.
  const [fieldPanelCollapsed, setFieldPanelCollapsed] = useFoldedWhileNarrow(isNarrowViewport);
  // Kept with the tab: its address carries the query it applied, which is no reason to open it on a reload.
  const [queryEditorCollapsed, setQueryEditorCollapsed] = useQueryTabState('traces.editorFolded', !initialUrlQuery.fields && !initialUrlQuery.sql);
  const [rangeRefreshAt, setRangeRefreshAt] = React.useState(() => Date.now());
  // Goes up when the reader asks for the results again: the histogram counts them again too.
  const [countEpoch, setCountEpoch] = React.useState(0);
  const appliedTraceQueryRef = React.useRef(traceUrlQueryStateKey(initialUrlQuery));
  const appliedTabParamRef = React.useRef<string | null>(urlTabParam);

  React.useEffect(() => {
    if (urlTabParam === appliedTabParamRef.current) return;
    appliedTabParamRef.current = urlTabParam;
    setTab(urlTab ?? DEFAULT_TRACE_TAB);
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
    setTab(urlTab ?? DEFAULT_TRACE_TAB);
  }, [searchParams, setQueryDraft, setQueryEditorCollapsed, setQueryMode, setSqlDraft, urlTab]);

  const range = React.useMemo(() => rangeFromWindow(effectiveTimeWindow, new Date(rangeRefreshAt)), [effectiveTimeWindow, rangeRefreshAt]);

  const streamsQuery = useQuery({ queryKey: ['streams', 'traces-fields'], queryFn: () => streamsApi.list(500), staleTime: 60_000 });
  const traceStreams = React.useMemo(
    () => (streamsQuery.data ?? []).filter((stream) => stream.type === 'traces' && streamsApi.isQueryable(stream)),
    [streamsQuery.data],
  );
  const primaryTraceDefinition = React.useMemo(() => selectTraceStream(traceStreams, requestedTraceStream), [requestedTraceStream, traceStreams]);
  const primaryTraceStream = primaryTraceDefinition?.name ?? '';
  // Fields come from the live traces-stream schema; an instance without a traces stream falls back to a minimal set.
  const traceFields = React.useMemo(() => deriveTraceFields(primaryTraceDefinition?.schema.fields ?? []), [primaryTraceDefinition]);
  const parsedQuery = React.useMemo(
    () => (queryMode === 'fields' ? parseTraceStatement(queryText, traceFields) : { filters: [], rejected: [] }),
    [queryMode, queryText, traceFields],
  );

  const topologyQuery = useQuery({ queryKey: ['web', 'topology', range.from, range.to], queryFn: () => webApi.topology(range.from, range.to) });
  const traceColumns = useTraceColumns(primaryTraceStream, traceFields);
  const listQuery = { orgId, range, queryMode, queryText, sqlText, parsedQuery, stream: primaryTraceStream };
  const traceList = useTraceList({ ...listQuery, sort: traceSort, rootColumns: traceColumns.rootFields });
  const spanList = useSpanList({ ...listQuery, sort: spanSort, enabled: tab === 'spans' });

  // The histograms count what the list queries match, so they are given the query the lists send.
  // A statement has no list query: its histogram is of what it returned.
  const countedQuery = React.useMemo(
    () =>
      queryMode === 'fields'
        ? { orgId, q: parsedQuery.q, filters: parsedQuery.filters, where: parsedQuery.where, epoch: countEpoch }
        : null,
    [countEpoch, orgId, parsedQuery, queryMode],
  );
  const traceHistogram = useTraceHistogram({ traces: traceList.traces, range, onTimeWindowChange: setTimeWindow, counted: countedQuery });
  const spanHistogram = useSpanHistogram({ spans: spanList.spans, range, onTimeWindowChange: setTimeWindow, counted: countedQuery });
  const traceCompletionItems = React.useMemo(() => traceQueryCompletions(traceFields, traceList.traces), [traceFields, traceList.traces]);

  // What a link to this view carries of the page's own: the view, the orders and the query that is applied.
  const shareSearch = React.useMemo(() => {
    const params = new URLSearchParams();
    for (const key of ['tab', 'view', 'stream', 'sort', 'spanSort', 'scope', 'by']) {
      const value = searchParams.get(key);
      if (value) params.set(key, value);
    }
    const statement = (queryMode === 'sql' ? sqlText : queryText).trim();
    if (statement) params.set(queryMode === 'sql' ? 'sql' : 'q', statement);
    return params;
  }, [queryMode, queryText, searchParams, sqlText]);

  // Writes on the params as they are by then, so a write that follows in the same step keeps this one.
  const selectTab = React.useCallback((next: TraceTab) => {
    setTab(next);
    setSearchParams((current) => {
      const params = new URLSearchParams(current);
      params.delete('view');
      if (next === DEFAULT_TRACE_TAB) params.delete('tab');
      else params.set('tab', next);
      appliedTabParamRef.current = params.get('tab');
      return params;
    }, { replace: true });
  }, [setSearchParams]);

  // The address says what the lists are of: a copy of it is the query applied, to send to someone.
  // What the page writes there is what it shows already, so it is not taken in again as a link.
  const publishQuery = React.useCallback((mode: TraceQueryMode, statement: string) => {
    setSearchParams((current) => {
      const next = writeTraceQueryParams(current, mode, statement);
      appliedTraceQueryRef.current = traceUrlQueryStateKey(traceUrlQueryStateFromParams(next));
      return next;
    }, { replace: true });
  }, [setSearchParams]);

  const applySearch = React.useCallback(() => {
    const statement = (queryMode === 'sql' ? sqlDraft : queryDraft).trim();
    if (queryMode === 'sql') setSqlText(statement);
    else setQueryText(statement);
    publishQuery(queryMode, statement);
    setQueryEditorCollapsed(true);
    if (tab === 'service-graph') selectTab(DEFAULT_TRACE_TAB);
  }, [publishQuery, queryDraft, queryMode, selectTab, setQueryEditorCollapsed, sqlDraft, tab]);

  // A field query applied from the page itself: a drill, a value picked on a chart.
  const applyFieldQuery = React.useCallback((statement: string) => {
    setQueryMode('fields');
    setQueryDraft(statement);
    setQueryText(statement);
    publishQuery('fields', statement);
  }, [publishQuery, setQueryDraft, setQueryMode]);

  const changeEditorMode = React.useCallback((nextMode: QueryEditorMode) => {
    setEditorMode(nextMode);
    setQueryEditorCollapsed(false);
  }, [setEditorMode, setQueryEditorCollapsed]);

  const openQueryEditor = React.useCallback(() => setQueryEditorCollapsed(false), [setQueryEditorCollapsed]);
  const runStatement = React.useCallback((mode: TraceQueryMode, statement: string) => {
    if (mode === 'sql') setSqlText(statement.trim());
    else setQueryText(statement.trim());
    publishQuery(mode, statement);
  }, [publishQuery]);
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
    () => ({ orgId, stream: primaryTraceStream, range: { start: Date.parse(range.from) * 1000, end: Date.parse(range.to) * 1000 } }),
    [orgId, primaryTraceStream, range.from, range.to],
  );
  // One field list for both lists. Only the Traces view puts a field of each trace's root beside it,
  // so only there is the eye offered.
  const fieldPanel = (
    <TraceFieldPanel
      traces={traceList.traces}
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
      {...(tab === 'traces' ? { visibleFields: traceColumns.rootFields, onToggleField: traceColumns.toggle } : {})}
      className="bg-[var(--functional-surface)]"
    />
  );

  const activeList = tab === 'spans' ? spanList.list : traceList.query;
  const traceListState = queryStateFor({ isLoading: traceList.query.isLoading, isError: traceList.query.isError, data: traceList.traces });
  const spanListState = queryStateFor({ isLoading: spanList.list.isLoading, isError: spanList.list.isError, data: spanList.spans });
  useQueryTabStatus({ running: activeList.isFetching, summary: (queryMode === 'sql' ? sqlText : queryText).trim().replace(/\s+/g, ' ') });
  const traceQueryCanRun = queryMode === 'fields' || Boolean(orgId && primaryTraceStream && sqlDraft.trim());

  const refreshCurrentView = React.useCallback(() => {
    setCountEpoch((epoch) => epoch + 1);
    if (timeWindow.mode === 'relative') {
      traceList.resetCursor();
      spanList.resetCursor();
      setRangeRefreshAt(Date.now());
      return;
    }
    void topologyQuery.refetch();
    if (queryMode === 'fields' && traceList.cursor !== null) traceList.resetCursor();
    else void traceList.query.refetch();
    void spanList.list.refetch();
  }, [queryMode, spanList, timeWindow.mode, topologyQuery, traceList]);

  const clearRecoveryFilters = React.useCallback(() => {
    setQueryMode('fields');
    setQueryDraft('');
    setQueryText('');
    setSqlDraft('');
    setSqlText('');
    traceList.resetCursor();
    spanList.resetCursor();
    publishQuery('fields', '');
  }, [publishQuery, setQueryDraft, setQueryMode, setSqlDraft, spanList, traceList]);
  const widenRecoveryRange = React.useCallback(() => {
    traceList.resetCursor();
    spanList.resetCursor();
    setTimeWindow(widenTimeWindow(effectiveTimeWindow));
  }, [effectiveTimeWindow, setTimeWindow, spanList, traceList]);
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
    onRetry: () => void activeList.refetch(),
    onClearFilters: clearRecoveryFilters,
    onWidenRange: widenRecoveryRange,
    docsHref: `https://docs.molesignal.com/${(i18n.resolvedLanguage ?? i18n.language).toLowerCase().startsWith('zh') ? 'zh-Hans' : 'en-US'}/traces`,
  };

  // A tag of a span added to the query from the drawer narrows the list it was opened from.
  const addTagToQuery = React.useCallback((key: string, value: string | number | boolean, operator: '=' | '!=') => {
    setQueryDraft((current) => appendAttributeClause(current, attributeClause(key, value, operator)));
    setQueryEditorCollapsed(false);
  }, [setQueryDraft, setQueryEditorCollapsed]);

  return (
    <div data-workspace="traces" className="flex h-full min-h-0 flex-col overflow-hidden bg-[var(--page-canvas)]">
      <TraceQueryPanel
        tab={tab}
        tabs={TRACE_TABS.map((tb) => ({
          id: tb.id,
          label: t(tb.labelKey),
          count: tb.id === 'traces' ? traceList.traces.length : tb.id === 'spans' && spanList.list.data ? spanList.spans.length : undefined,
        }))}
        editorMode={editorMode}
        queryMode={queryMode}
        traceFields={traceFields}
        builderField={draft.builderField}
        queryDraft={queryDraft}
        queryText={queryText}
        sqlDraft={sqlDraft}
        parsedQuery={parsedQuery}
        completionItems={traceCompletionItems}
        running={activeList.isFetching}
        canRun={traceQueryCanRun}
        collapsed={queryEditorCollapsed}
        sqlPlaceholder={traceSqlPlaceholder(primaryTraceStream)}
        onTabChange={selectTab}
        onEditorModeChange={changeEditorMode}
        onCodeDraftChange={draft.changeCodeDraft}
        onBuilderDraftChange={draft.changeBuilderDraft}
        onBuilderFieldHandled={() => setBuilderField(null)}
        onCollapsedChange={setQueryEditorCollapsed}
        onApplySearch={applySearch}
        onRefresh={refreshCurrentView}
        shareSearch={shareSearch}
      />

      {/* Query workspaces fill the remaining viewport. Result panes own their scrolling so empty
          states stretch vertically and pagination stays pinned to the bottom edge. */}
      <div className="min-h-0 flex-1 overflow-hidden bg-[var(--page-canvas)] pl-[var(--pane-gutter-start,20px)] pr-[var(--pane-gutter-end,20px)]">
        <AttributeFilterContext.Provider value={queryMode === 'sql' ? null : addTagToQuery}>
          {tab === 'traces' && (
            <TraceTableExplorer
              fieldPanel={fieldPanel}
              traceList={traceList.pagedTraces}
              loadedTraceCount={traceList.traces.length}
              sort={traceSort}
              sortEnabled={queryMode === 'fields'}
              listState={traceListState}
              listError={traceList.query.error}
              recovery={recovery}
              recoveryCopy={recoveryCopy}
              columns={traceColumns}
              pagination={traceList.pagination}
              histogram={traceHistogram}
              onSortChange={(next) => {
                setSearchParams(writeTraceListSort(searchParams, next), { replace: true });
                traceList.resetCursor();
              }}
            />
          )}

          {tab === 'spans' && (
            <SpanListExplorer
              fieldPanel={fieldPanel}
              spans={spanList.spans}
              sort={spanSort}
              sortEnabled={queryMode === 'fields'}
              listState={spanListState}
              listError={spanList.list.error}
              recovery={recovery}
              recoveryCopy={recoveryCopy}
              pagination={spanList.pagination}
              histogram={spanHistogram}
              onSortChange={(next) => {
                setSearchParams(writeSpanListSort(searchParams, next), { replace: true });
                spanList.resetCursor();
              }}
            />
          )}
        </AttributeFilterContext.Provider>

        {tab === 'service-graph' && (
          <ServiceGraphPanel
            range={range}
            data={topologyQuery.data}
            isLoading={topologyQuery.isLoading}
            isError={topologyQuery.isError}
            error={topologyQuery.error}
            onServiceSelect={(serviceId) => {
              applyFieldQuery(`service.name = '${quoteTraceValue(serviceId)}'`);
              selectTab('spans');
            }}
          />
        )}

        {tab === 'red' && (
          <TraceRedView
            orgId={orgId}
            range={range}
            epoch={countEpoch}
            queryMode={queryMode}
            queryText={queryText}
            parsedQuery={parsedQuery}
            fields={traceFields}
            onQueryChange={applyFieldQuery}
            onOpenSpans={(statement) => {
              applyFieldQuery(statement);
              selectTab('spans');
            }}
            onTimeWindowChange={setTimeWindow}
            onWidenRange={widenRecoveryRange}
            onClearQuery={clearRecoveryFilters}
          />
        )}
      </div>
    </div>
  );
}
