import { useMutation, useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useSearchParams } from 'react-router-dom';

import {
  fetchMetricCatalog,
  type MetricCatalogEntry,
} from '@/api/metricsCatalog';
import * as queryApi from '@/api/query';
import { resolveMetricType } from '@/lib/metricTypes';
import { useCursorPagination } from '@/pagination/useCursorPagination';
import { useActionAccess } from '@/product/actionAccess';
import { PageHeader } from '@/shell/PageHeader';
import { useAuthStore } from '@/stores/auth';
import { useFiltersStore } from '@/stores/useFiltersStore';
import {
  resolveWindow,
  type TimeWindow,
  useTimeStore,
} from '@/stores/useTimeStore';

import { PromqlBuilderPanel } from './metrics/builder/PromqlBuilderPanel';
import { ExploreQuerySection } from './metrics/ExploreQuerySection';
import { MetricCatalogPanel } from './metrics/MetricCatalogPanel';
import {
  buildPromqlCompletionItems,
  defaultPromqlForMetric,
  findMetricName,
  injectPromqlMatchers,
  isRateQuery,
  metricChartTitle,
  metricQueryUnit,
  type MetricsDrawStyle,
  type MetricsStackMode,
  requestedPromqlFromParams,
  timeWindowKey,
} from './metrics/model';
import {
  DEFAULT_METRICS_QUERY_OPTIONS,
  isValidMetricsStep,
  metricsQueryLimit,
  type MetricsQueryOptions,
} from './metrics/queryOptions/model';
import { MetricsExploreResults } from './metrics/results';
import { useMetricSeriesPresentation } from './metrics/results/useMetricSeriesPresentation';

/**
 * Prometheus Explore workspace. The route owns query/data state while the
 * query editor, metric browser and result views remain isolated components.
 */
export function Metrics() {
  const { i18n, t } = useTranslation('metrics');
  const dashboardCreateAccess = useActionAccess({
    permission: 'dashboards.create',
  });
  const nav = useNavigate();
  const [searchParams] = useSearchParams();
  const requestedPromql = requestedPromqlFromParams(searchParams);
  const [promql, setPromql] = React.useState(requestedPromql);
  const [lastExecutedPromql, setLastExecutedPromql] = React.useState<
    string | null
  >(null);
  const [executionVersion, setExecutionVersion] = React.useState(0);
  const [filter, setFilter] = React.useState('');
  const [metricBrowserOpen, setMetricBrowserOpen] = React.useState(false);
  const [queryMode, setQueryMode] = React.useState<'code' | 'builder'>('code');
  const [queryEditorCollapsed, setQueryEditorCollapsed] = React.useState(false);
  const [timezone, setTimezone] = React.useState('');
  const [queryOptions, setQueryOptions] = React.useState<MetricsQueryOptions>(
    DEFAULT_METRICS_QUERY_OPTIONS,
  );
  const [chartDrawStyle, setChartDrawStyle] =
    React.useState<MetricsDrawStyle>('line');
  const [chartStackMode, setChartStackMode] =
    React.useState<MetricsStackMode>('none');
  const [chartZoomOrigin, setChartZoomOrigin] =
    React.useState<TimeWindow | null>(null);

  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const timeWindow = useTimeStore((state) => state.window);
  const setTimeWindow = useTimeStore((state) => state.setWindow);
  const globalFilters = useFiltersStore((state) => state.filters);
  const chartZoomWindowKey = React.useRef<string | null>(null);
  const lastRequestedPromql = React.useRef(requestedPromql);
  const lastRunStatement = React.useRef(promql);

  const metricCatalogContextKey = React.useMemo(
    () => JSON.stringify({ orgId, filter: filter.trim().toLowerCase() }),
    [filter, orgId],
  );
  const metricCatalogPagination = useCursorPagination({
    contextKey: metricCatalogContextKey,
    defaultPageSize: 20,
  });

  const run = useMutation({
    mutationFn: (statementOverride?: string) => {
      const resolvedWindow = resolveWindow(useTimeStore.getState().window);
      const statement = injectPromqlMatchers(
        statementOverride ?? promql,
        useFiltersStore.getState().filters,
      );
      lastRunStatement.current = statement;
      return queryApi.runQuery({
        org_id: orgId,
        language: 'promql',
        statement,
        time_range: {
          start: resolvedWindow.from.getTime() * 1000,
          end: resolvedWindow.to.getTime() * 1000,
        },
        limit: metricsQueryLimit(
          queryOptions,
          resolvedWindow.from.getTime() * 1000,
          resolvedWindow.to.getTime() * 1000,
        ),
      });
    },
    onSuccess: () => {
      setLastExecutedPromql(lastRunStatement.current.trim());
      setExecutionVersion((current) => current + 1);
    },
  });

  React.useEffect(() => {
    if (!requestedPromql || requestedPromql === lastRequestedPromql.current) {
      return;
    }
    lastRequestedPromql.current = requestedPromql;
    setPromql(requestedPromql);
    if (orgId) run.mutate(requestedPromql);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requestedPromql, orgId]);

  React.useEffect(() => {
    if (!orgId || !promql.trim() || !isValidMetricsStep(queryOptions.step)) {
      return;
    }
    run.mutate(undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [orgId, timeWindow, globalFilters]);

  const catalog = useQuery({
    queryKey: [
      'metrics-catalog',
      orgId,
      filter.trim().toLowerCase(),
      metricCatalogPagination.pageSize,
      metricCatalogPagination.cursor,
    ],
    queryFn: () =>
      fetchMetricCatalog({
        ...(filter.trim() ? { q: filter.trim() } : {}),
        limit: metricCatalogPagination.pageSize,
        ...(metricCatalogPagination.cursor
          ? { cursor: metricCatalogPagination.cursor }
          : {}),
      }),
    enabled: Boolean(orgId),
  });
  const builderCatalog = useQuery({
    queryKey: ['metrics-builder-catalog', orgId],
    queryFn: () => fetchMetricCatalog({ limit: 100 }),
    enabled: Boolean(orgId && queryMode === 'builder'),
  });
  const capabilities = useQuery({
    queryKey: ['promql-capabilities'],
    queryFn: queryApi.fetchPromqlCapabilities,
    enabled: Boolean(orgId),
    staleTime: Number.POSITIVE_INFINITY,
  });
  const completionItems = React.useMemo(
    () =>
      buildPromqlCompletionItems(
        capabilities.data,
        catalog.data?.items ?? [],
      ),
    [capabilities.data, catalog.data?.items],
  );

  const selectedMetricName = React.useMemo(
    () => findMetricName(promql, catalog.data?.items ?? []),
    [catalog.data?.items, promql],
  );
  const selectedMetric = React.useMemo(
    () =>
      catalog.data?.items.find(
        (metric) => metric.name === selectedMetricName,
      ) ?? null,
    [catalog.data?.items, selectedMetricName],
  );
  const pickMetric = React.useCallback(
    (metric: MetricCatalogEntry) => {
      const nextPromql = defaultPromqlForMetric(metric);
      setPromql(nextPromql);
      setMetricBrowserOpen(false);
      run.mutate(nextPromql);
    },
    [run],
  );

  const chartWindow = React.useMemo(
    () => resolveWindow(timeWindow),
    [timeWindow],
  );
  const chartXDomain = React.useMemo<[number, number]>(
    () => [
      chartWindow.from.getTime() * 1000,
      chartWindow.to.getTime() * 1000,
    ],
    [chartWindow],
  );
  const chartUnit = React.useMemo(
    () => metricQueryUnit(lastExecutedPromql ?? promql, selectedMetricName),
    [lastExecutedPromql, promql, selectedMetricName],
  );
  const {
    metricSeries,
    chartSeries,
    quality,
  } = useMetricSeriesPresentation({
    result: run.data,
    metricName: selectedMetricName,
    legend: queryOptions.legend,
    unit: chartUnit,
    xDomain: chartXDomain,
  });
  const exemplarsEnabled =
    queryOptions.exemplars && queryOptions.type === 'range';
  const exemplars = useQuery({
    queryKey: [
      'prometheus-exemplars',
      orgId,
      lastExecutedPromql,
      chartXDomain[0],
      chartXDomain[1],
      executionVersion,
      exemplarsEnabled,
    ],
    queryFn: () =>
      queryApi.fetchPrometheusExemplars({
        query: lastExecutedPromql!,
        startMicros: chartXDomain[0],
        endMicros: chartXDomain[1],
      }),
    enabled: Boolean(
      orgId && lastExecutedPromql && exemplarsEnabled,
    ),
    retry: false,
  });

  React.useEffect(() => {
    if (
      chartZoomOrigin &&
      chartZoomWindowKey.current &&
      timeWindowKey(timeWindow) !== chartZoomWindowKey.current
    ) {
      chartZoomWindowKey.current = null;
      setChartZoomOrigin(null);
    }
  }, [chartZoomOrigin, timeWindow]);

  const selectChartTimeRange = React.useCallback(
    ({ from, to }: { from: number; to: number }) => {
      const nextWindow: TimeWindow = {
        from: new Date(from / 1000).toISOString(),
        to: new Date(to / 1000).toISOString(),
        mode: 'absolute',
      };
      setChartZoomOrigin(
        (origin) => origin ?? useTimeStore.getState().window,
      );
      chartZoomWindowKey.current = timeWindowKey(nextWindow);
      setTimeWindow(nextWindow);
    },
    [setTimeWindow],
  );
  const resetChartTimeRange = React.useCallback(() => {
    if (!chartZoomOrigin) return;
    chartZoomWindowKey.current = timeWindowKey(chartZoomOrigin);
    setTimeWindow(chartZoomOrigin);
    setChartZoomOrigin(null);
  }, [chartZoomOrigin, setTimeWindow]);

  const counterRateQuery =
    selectedMetric !== null &&
    resolveMetricType(selectedMetric) === 'counter' &&
    isRateQuery(lastExecutedPromql ?? promql);
  const chartTitle = metricChartTitle(
    selectedMetricName,
    lastExecutedPromql ?? promql,
    t,
  );

  const canRun = Boolean(
    orgId &&
      promql.trim() &&
      isValidMetricsStep(queryOptions.step) &&
      !run.isPending,
  );
  const promqlDirty = Boolean(
    promql.trim() && promql.trim() !== lastExecutedPromql,
  );
  const language = i18n.resolvedLanguage ?? i18n.language;
  const promqlDocsLocale = language.toLowerCase().startsWith('zh')
    ? 'zh-Hans'
    : 'en-US';
  const promqlDocsHref = `https://docs.molesignal.io/${promqlDocsLocale}/query/promql-subset`;
  const timeRangeSeconds =
    (chartWindow.to.getTime() - chartWindow.from.getTime()) / 1000;

  const addToDashboard = React.useCallback(() => {
    const statement = promql.trim();
    if (!dashboardCreateAccess.allowed || !statement) return;
    const params = new URLSearchParams({
      panelQuery: statement,
      panelTitle:
        statement.length > 72 ? `${statement.slice(0, 69)}...` : statement,
      panelType: chartDrawStyle === 'bar' ? 'bar' : 'line',
    });
    nav(`/dashboards/new/edit?${params.toString()}`);
  }, [chartDrawStyle, dashboardCreateAccess.allowed, nav, promql]);
  const viewRawCounter = React.useCallback(() => {
    if (!selectedMetricName) return;
    setPromql(selectedMetricName);
    run.mutate(selectedMetricName);
  }, [run, selectedMetricName]);
  const inspectMetricType = React.useCallback(() => {
    setMetricBrowserOpen(true);
    if (selectedMetricName) setFilter(selectedMetricName);
  }, [selectedMetricName]);

  return (
    <div
      className="flex h-[calc(100vh-var(--topbar-h)-var(--contextbar-h,0px))] min-h-0 flex-col overflow-hidden"
      data-testid="metrics-page"
    >
      <PageHeader
        title={t('explore.title')}
        subtitle={t('explore.subtitle')}
        className="shrink-0"
      />
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <ExploreQuerySection
          promql={promql}
          completionItems={completionItems}
          collapsed={queryEditorCollapsed}
          mode={queryMode}
          builder={(
            <PromqlBuilderPanel
              expression={promql}
              metrics={builderCatalog.data?.items ?? []}
              pending={builderCatalog.isPending}
              error={builderCatalog.isError ? builderCatalog.error : null}
              capabilities={capabilities.data}
              onExpressionChange={setPromql}
            />
          )}
          canRun={canRun}
          running={run.isPending}
          dirty={promqlDirty}
          timezone={timezone}
          options={queryOptions}
          addToDashboardDisabled={
            !promql.trim() || dashboardCreateAccess.disabled
          }
          addToDashboardDisabledReason={
            dashboardCreateAccess.disabled
              ? dashboardCreateAccess.reason
              : undefined
          }
          promqlDocsHref={promqlDocsHref}
          onPromqlChange={setPromql}
          onCollapsedChange={setQueryEditorCollapsed}
          onModeChange={(mode) => {
            setQueryMode(mode);
            setQueryEditorCollapsed(false);
          }}
          onOpenMetricBrowser={() => setMetricBrowserOpen(true)}
          onTimezoneChange={setTimezone}
          onOptionsChange={setQueryOptions}
          onRun={() => run.mutate(undefined)}
          onRefresh={() => run.mutate(undefined)}
          onAddToDashboard={addToDashboard}
        />

        <MetricCatalogPanel
          metrics={catalog.data?.items ?? []}
          pending={catalog.isPending}
          error={catalog.isError ? catalog.error : null}
          selectedMetricName={selectedMetricName}
          filter={filter}
          open={metricBrowserOpen}
          pageSize={metricCatalogPagination.pageSize}
          hasPrevious={Boolean(catalog.data?.previous_cursor)}
          hasNext={Boolean(catalog.data?.next_cursor)}
          onOpenChange={setMetricBrowserOpen}
          onFilterChange={setFilter}
          onPickMetric={pickMetric}
          onPrevious={() => metricCatalogPagination.goPrevious(catalog.data)}
          onNext={() => metricCatalogPagination.goNext(catalog.data)}
          onPageSizeChange={metricCatalogPagination.setPageSize}
        />

        <MetricsExploreResults
          query={{
            result: run.data,
            error: run.isError ? run.error : null,
            pending: run.isPending,
            promql,
            executedPromql: lastExecutedPromql,
            chartTitle,
          }}
          series={{
            metricSeries,
            chartSeries,
            quality,
            ...(chartUnit ? { unit: chartUnit } : {}),
            counterRateQuery,
          }}
          chart={{
            xDomain: chartXDomain,
            timezone,
            drawStyle: chartDrawStyle,
            stackMode: chartStackMode,
            zoomed: chartZoomOrigin !== null,
            onDrawStyleChange: setChartDrawStyle,
            onStackModeChange: setChartStackMode,
            onRangeSelect: selectChartTimeRange,
            onRangeReset: resetChartTimeRange,
          }}
          exemplars={{
            series: exemplarsEnabled ? exemplars.data?.data ?? [] : [],
            ...(exemplarsEnabled && exemplars.data?.warnings?.length
              ? { warning: exemplars.data.warnings.join(' · ') }
              : {}),
            ...(exemplarsEnabled && exemplars.isError
              ? { error: t('explore.exemplars.query_error') }
              : {}),
          }}
          timeRangeSeconds={timeRangeSeconds}
          language={language}
          preferredView={
            queryOptions.format === 'table' ? 'table' : 'graph'
          }
          onPreferredViewChange={(view) =>
            setQueryOptions((current) => ({
              ...current,
              format: view === 'table' ? 'table' : 'time_series',
            }))
          }
          onViewRawCounter={viewRawCounter}
          onInspectMetricType={inspectMetricType}
        />
      </div>
    </div>
  );
}
