import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import type { RumField } from '@/api/rum';
import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { toApiError } from '@/lib/http';
import type { QueryEditorMode } from '@/shell/query/ModeSwitch';
import { QueryRecoveryState, type QueryRecoveryActions, type QueryRecoveryCopy } from '@/shell/query/RecoveryState';
import { queryStateFor } from '@/shell/query/State';
import { useFoldedWhileNarrow, useQueryTabSearchParams, useQueryTabState, useQueryTabStatus } from '@/shell/queryTabs';
import { toast } from '@/shell/ui/sonner';
import { isNarrowViewport } from '@/shell/useViewportWidth';
import { useAuthStore } from '@/stores/auth';
import { resolveWindow, type TimeWindow } from '@/stores/useTimeStore';

import { rumDocumentationUrl } from '../documentation';
import { RumGeoCredit } from '../RumLayout';
import { rumQueryCompletions } from './completions';
import { RumAddToDashboard } from './export/AddToDashboard';
import { rumAlertPlan } from './export/alertSource';
import { RumNewAlert } from './export/NewAlert';
import { rumPanelPlan } from './export/panelModel';
import { exportRumRows } from './exportRows';
import { RumFieldPanel } from './FieldPanel';
import { NO_STATEMENT, parseRumStatement } from './fieldQuery';
import { RumFirstRun } from './FirstRun';
import { useRumHistogram } from './Histogram';
import { RUM_TABS, type RumTab } from './model';
import { useRumQueryDraft } from './queryDraft';
import { RumQueryPanel } from './QueryPanel';
import { viewSearchOf } from './savedViews/model';
import { RumSavedViews } from './savedViews/SavedViews';
import { useRumTimeWindow } from './timeWindow';
import { rumQueryFromParams, rumTabFromParams, writeRumQueryParams, writeRumTabParams } from './urlState';
import { useRumFieldFilters } from './useFieldFilters';
import { useRumColumns } from './useRumColumns';
import { useRumList } from './useRumList';
import { useRumSchema } from './useRumSchema';
import { canFilterRumValue } from './valueFilter';
import { copyRumVizParams } from './viz/address';
import { RumAggregateRow } from './viz/AggregateRow';
import type { VizFilters } from './viz/labels';
import { vizStatus } from './viz/status';
import { useRumVizState } from './viz/useRumVizState';
import { RumVizResults } from './viz/VizResults';
import { RumVizSwitch } from './viz/VizSwitch';
import { RumWorkspace } from './Workspace';

function rangeFromWindow(window: TimeWindow, now: Date): { from: string; to: string } {
  const resolved = resolveWindow(window, now);
  return { from: resolved.from.toISOString(), to: resolved.to.toISOString() };
}

const NO_FIELDS: RumField[] = [];

/**
 * The RUM explorer: the rows real users left, of one type at a time, found by a field query
 * (written in Code or put together in the Builder), counted over time, and opened in a drawer.
 * Until the first application exists there is nothing to explore, and the page says how to start.
 */
export function RumExplorer() {
  const applications = useQuery({
    queryKey: ['rum', 'applications'],
    queryFn: () => rumApi.listApplications(),
  });

  if (applications.isSuccess && applications.data.length === 0) return <RumFirstRun />;

  return <RumExplorerPage />;
}

function RumExplorerPage() {
  const { t, i18n } = useTranslation('rum');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  // Read already to know whether there is anything to explore: a rule is about one of them.
  const applications = useQuery({ queryKey: ['rum', 'applications'], queryFn: () => rumApi.listApplications() });
  // The page can be one of several query tabs: its params and its time range are the tab's own.
  const [searchParams, setSearchParams] = useQueryTabSearchParams();
  const { timeWindow, effectiveTimeWindow, setTimeWindow } = useRumTimeWindow(searchParams);
  const tab = rumTabFromParams(searchParams);
  const initialQuery = React.useRef(rumQueryFromParams(searchParams)).current;
  const draft = useRumQueryDraft(initialQuery);
  const { editorMode, setEditorMode, builderField, setBuilderField, queryDraft, setQueryDraft } = draft;
  const [queryText, setQueryText] = React.useState(initialQuery);
  const [fieldFilter, setFieldFilter] = React.useState('');
  // Folded in a narrow window, and for as long as the page has a narrow share of a split one.
  const [fieldPanelCollapsed, setFieldPanelCollapsed] = useFoldedWhileNarrow(isNarrowViewport);
  // Kept with the tab: its address carries the query it applied, which is no reason to open it on a reload.
  const [queryEditorCollapsed, setQueryEditorCollapsed] = useQueryTabState('rum.editorFolded', !initialQuery);
  const [rangeRefreshAt, setRangeRefreshAt] = React.useState(() => Date.now());
  // Goes up when the reader asks for the results again: the histogram and the values count them again too.
  const [epoch, setEpoch] = React.useState(0);
  const appliedQueryRef = React.useRef(initialQuery);

  // A link, or the history, changes the query in the address: take it in. What the page writes
  // there itself is what it shows already.
  React.useEffect(() => {
    const next = rumQueryFromParams(searchParams);
    if (next === appliedQueryRef.current) return;
    appliedQueryRef.current = next;
    setQueryDraft(next);
    setQueryText(next);
    setQueryEditorCollapsed(next === '');
  }, [searchParams, setQueryDraft, setQueryEditorCollapsed]);

  const range = React.useMemo(
    () => rangeFromWindow(effectiveTimeWindow, new Date(rangeRefreshAt)),
    [effectiveTimeWindow, rangeRefreshAt],
  );

  const schema = useRumSchema(tab);
  const fields = schema.data?.fields ?? NO_FIELDS;
  const parsed = React.useMemo(
    () => (schema.isSuccess ? parseRumStatement(queryText, fields) : NO_STATEMENT),
    [fields, queryText, schema.isSuccess],
  );
  const columns = useRumColumns(tab, fields);
  const rangeMicros = React.useMemo(
    () => ({ from: Date.parse(range.from) * 1_000, to: Date.parse(range.to) * 1_000 }),
    [range.from, range.to],
  );
  // How the rows are drawn: as themselves, or as numbers over them. Only what is in front is read.
  const viz = useRumVizState({
    orgId,
    tab,
    fields,
    ready: schema.isSuccess,
    searchParams,
    setSearchParams,
    range: rangeMicros,
    parsed,
    epoch,
  });
  const listInFront = viz.viz === 'list';
  const list = useRumList({
    orgId,
    tab,
    range,
    statement: queryText,
    parsed,
    columns: columns.names,
    enabled: Boolean(orgId) && schema.isSuccess && listInFront,
  });
  const counted = React.useMemo(
    () => ({ orgId, tab, filters: parsed.filters, freeText: parsed.freeText, epoch }),
    [epoch, orgId, parsed, tab],
  );
  const histogram = useRumHistogram({ rows: list.rows, range, onTimeWindowChange: setTimeWindow, counted });
  const completions = React.useMemo(() => rumQueryCompletions(fields), [fields]);

  // What a link to this view carries of the page's own: the type and the query that is applied.
  const shareSearch = React.useMemo(() => {
    const params = new URLSearchParams();
    const type = searchParams.get('type');
    if (type) params.set('type', type);
    if (queryText.trim()) params.set('q', queryText.trim());
    copyRumVizParams(searchParams, params);
    return params;
  }, [queryText, searchParams]);

  // What a saved view keeps of the same: what shows it, and a range that moves.
  const savedSearch = React.useMemo(() => viewSearchOf(shareSearch, searchParams.get('time')), [searchParams, shareSearch]);
  const openSavedView = React.useCallback(
    (search: string) => setSearchParams(() => new URLSearchParams(search), { replace: false }),
    [setSearchParams],
  );

  // What is drawn, as a panel of a dashboard and as the figure of an alert rule.
  const panelPlan = React.useMemo(
    () => rumPanelPlan({ t, tab, viz: viz.viz, spec: viz.spec, filters: parsed.filters, freeText: parsed.freeText }),
    [parsed.filters, parsed.freeText, t, tab, viz.spec, viz.viz],
  );
  const alertPlan = React.useMemo(
    () =>
      rumAlertPlan({
        tab,
        viz: viz.viz,
        spec: viz.spec,
        statement: queryText,
        filters: parsed.filters,
        freeText: parsed.freeText,
        applications: (applications.data ?? []).map((application) => application.id),
      }),
    [applications.data, parsed.filters, parsed.freeText, queryText, tab, viz.spec, viz.viz],
  );

  const selectTab = React.useCallback(
    (next: RumTab) => {
      setSearchParams((current) => writeRumTabParams(current, next), { replace: true });
    },
    [setSearchParams],
  );

  // The address says what the list is of: a copy of it is the query applied, to send to someone.
  const publishQuery = React.useCallback(
    (statement: string) => {
      setSearchParams(
        (current) => {
          const next = writeRumQueryParams(current, statement);
          appliedQueryRef.current = rumQueryFromParams(next);
          return next;
        },
        { replace: true },
      );
    },
    [setSearchParams],
  );

  const applySearch = React.useCallback(() => {
    const statement = queryDraft.trim();
    setQueryText(statement);
    publishQuery(statement);
    setQueryEditorCollapsed(true);
  }, [publishQuery, queryDraft, setQueryEditorCollapsed]);

  const runStatement = React.useCallback(
    (statement: string) => {
      setQueryText(statement.trim());
      publishQuery(statement);
    },
    [publishQuery],
  );

  const openQueryEditor = React.useCallback(() => setQueryEditorCollapsed(false), [setQueryEditorCollapsed]);
  const changeEditorMode = React.useCallback(
    (mode: QueryEditorMode) => {
      setEditorMode(mode);
      setQueryEditorCollapsed(false);
    },
    [setEditorMode, setQueryEditorCollapsed],
  );

  const { insertField, filterValue, filterValues, isValueFiltered } = useRumFieldFilters({
    editorMode,
    queryDraft,
    setBuilderField,
    setQueryDraft,
    onDraftStarted: openQueryEditor,
    onRun: runStatement,
  });

  const valueSource = React.useMemo(
    () => ({ orgId, tab, range: rangeMicros, parsed, epoch }),
    [epoch, orgId, parsed, rangeMicros, tab],
  );

  // What a group of a drawing can do to the query: the same filters the field list makes.
  const fieldByName = React.useMemo(() => new Map(fields.map((field) => [field.name, field])), [fields]);
  const vizFilters = React.useMemo<VizFilters>(
    () => ({
      can: (name, value) => {
        const field = fieldByName.get(name);
        return field !== undefined && canFilterRumValue(field, value, '=');
      },
      isFiltered: (name, value) => {
        const field = fieldByName.get(name);
        return field !== undefined && isValueFiltered(field, value, '=');
      },
      toggle: (name, value) => {
        const field = fieldByName.get(name);
        if (field) filterValue(field, value, '=');
      },
      apply: (entries) =>
        filterValues(
          entries.flatMap(({ field: name, value, op }) => {
            const field = fieldByName.get(name);
            return field && canFilterRumValue(field, value, op) ? [{ field, value, op }] : [];
          }),
        ),
    }),
    [fieldByName, filterValue, filterValues, isValueFiltered],
  );

  const language = i18n.resolvedLanguage ?? i18n.language;
  const fieldPanel = (
    <RumFieldPanel
      fields={fields}
      source={valueSource}
      language={language}
      visibleColumns={columns.names}
      onToggleColumn={columns.toggle}
      fieldFilter={fieldFilter}
      collapsed={fieldPanelCollapsed}
      onFieldFilterChange={setFieldFilter}
      onCollapsedChange={setFieldPanelCollapsed}
      onInsertField={insertField}
      isValueFiltered={isValueFiltered}
      onFilterValue={filterValue}
      className="bg-[var(--functional-surface)]"
    />
  );

  const listState = queryStateFor({
    isLoading: list.query.isLoading || schema.isLoading,
    isError: list.query.isError || schema.isError,
    data: list.rows,
  });
  const running = list.query.isFetching || viz.query.isFetching || viz.distribution.isFetching;
  useQueryTabStatus({ running, summary: queryText.trim().replace(/\s+/g, ' ') });

  const refreshCurrentView = React.useCallback(() => {
    setEpoch((current) => current + 1);

    if (timeWindow.mode === 'relative') {
      list.resetCursor();
      setRangeRefreshAt(Date.now());
      return;
    }

    // The numbers are asked again by their key, which carries the epoch; the list is not.
    if (listInFront) void list.query.refetch();
  }, [list, listInFront, timeWindow.mode]);

  const exportList = React.useCallback(async () => {
    try {
      const { rows, more } = await exportRumRows({ tab, range: rangeMicros, parsed, columns: columns.names });

      if (more) toast.info(t('explorer.results.export_more', { count: rows }));
    } catch (error) {
      toast.error(toApiError(error).message);
    }
  }, [columns.names, parsed, rangeMicros, t, tab]);

  const clearRecoveryFilters = React.useCallback(() => {
    setQueryDraft('');
    setQueryText('');
    list.resetCursor();
    publishQuery('');
  }, [list, publishQuery, setQueryDraft]);

  const widenRecoveryRange = React.useCallback(() => {
    list.resetCursor();
    setTimeWindow(widenTimeWindow(effectiveTimeWindow));
  }, [effectiveTimeWindow, list, setTimeWindow]);

  const recoveryCopy: QueryRecoveryCopy = {
    errorTitle: t('explorer.recovery.query_failed'),
    emptyTitle: t('explorer.recovery.no_data_title'),
    emptyDescription: t('explorer.recovery.no_data_description'),
    clearFiltersLabel: t('explorer.recovery.clear_filters'),
    widenRangeLabel: t('explorer.recovery.expand_time'),
    docsLabel: t('explorer.recovery.query_docs'),
  };
  const recovery: QueryRecoveryActions = {
    hasFilters: Boolean(queryText.trim()),
    onRetry: () =>
      void (schema.isError
        ? schema.refetch()
        : listInFront
          ? list.query.refetch()
          : viz.viz === 'distribution'
            ? viz.distribution.refetch()
            : viz.query.refetch()),
    onClearFilters: clearRecoveryFilters,
    onWidenRange: widenRecoveryRange,
    docsHref: rumDocumentationUrl(language, 'explorer'),
  };

  return (
    <div data-workspace="rum" className="flex h-full min-h-0 flex-col overflow-hidden bg-[var(--page-canvas)]">
      <RumQueryPanel
        tab={tab}
        tabs={RUM_TABS.map((type) => ({
          id: type,
          label: t(`explorer.types.${type}`),
          count: type === tab && listInFront && list.query.data ? list.rows.length : undefined,
        }))}
        editorMode={editorMode}
        fields={fields}
        builderField={builderField}
        queryDraft={queryDraft}
        queryText={queryText}
        parsedQuery={parsed}
        completionItems={completions}
        running={running}
        collapsed={queryEditorCollapsed}
        onTabChange={selectTab}
        onEditorModeChange={changeEditorMode}
        onDraftChange={setQueryDraft}
        onBuilderFieldHandled={() => setBuilderField(null)}
        onCollapsedChange={setQueryEditorCollapsed}
        onApplySearch={applySearch}
        onRefresh={refreshCurrentView}
        shareSearch={shareSearch}
        savedViews={<RumSavedViews search={savedSearch} onOpen={openSavedView} />}
        exports={
          <>
            <RumAddToDashboard
              plan={panelPlan}
              timeWindow={effectiveTimeWindow}
              range={rangeMicros}
              bucket={viz.spec?.bucket ?? null}
              ready={schema.isSuccess}
            />
            <RumNewAlert plan={alertPlan} tab={tab} ready={schema.isSuccess} />
          </>
        }
        aggregation={
          viz.spec ? <RumAggregateRow spec={viz.spec} fields={fields} range={viz.range} onChange={viz.onSettingsChange} /> : null
        }
      />

      {/* The workspace fills the remaining viewport. The result pane owns its scrolling so an empty
          state stretches vertically and pagination stays pinned to the bottom edge. */}
      <div className="min-h-0 flex-1 overflow-hidden bg-[var(--page-canvas)] pl-[var(--pane-gutter-start,20px)] pr-[var(--pane-gutter-end,20px)]">
        <RumWorkspace
          tab={tab}
          fieldPanel={fieldPanel}
          fields={fields}
          columns={columns.columns}
          rows={list.rows}
          listState={listState}
          listError={schema.error ?? list.query.error}
          recovery={recovery}
          recoveryCopy={recoveryCopy}
          pagination={list.pagination}
          histogram={histogram}
          arranged={columns.arranged}
          onResetColumns={columns.reset}
          vizSwitch={<RumVizSwitch viz={viz.viz} canMap={viz.canMap} onChange={viz.onVizChange} />}
          vizBody={
            listInFront ? null : schema.isError ? (
              // Without the fields of the type there is nothing to ask for.
              <QueryRecoveryState
                state="error"
                error={schema.error}
                copy={recoveryCopy}
                recovery={recovery}
                className="h-full min-h-0"
                testId="rum-viz-state"
              />
            ) : (
              <RumVizResults
                model={viz}
                tab={tab}
                language={language}
                filters={vizFilters}
                recovery={recovery}
                recoveryCopy={recoveryCopy}
                onRangeSelect={histogram.onRangeSelect}
              />
            )
          }
          vizStatus={
            viz.spec && viz.request
              ? vizStatus(t, language, { spec: viz.spec, request: viz.request, answer: viz.query.data, distribution: viz.distribution.data, range: viz.range })
              : ''
          }
          onExport={exportList}
        />
      </div>
      {/* Every row says where its client was: the lookup's source is credited under them. */}
      <RumGeoCredit className="shrink-0 px-[var(--pane-gutter-start,20px)] py-[4px]" />
    </div>
  );
}
