import { useQuery } from '@tanstack/react-query';
import {
  Activity,
  AlertTriangle,
  CheckCircle2,
  CircleOff,
  ExternalLink,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import {
  apmApi,
  apmQueryKeys,
  type DependencySummary,
  type ErrorSummary,
  type ServiceSummary,
  type TraceExemplar,
  type TransactionSummary,
} from '@/api/apm';

import {
  ApmPageFrame,
  HealthDot,
  QueryBoundary,
  RedKpis,
  Section,
  SectionLink,
  TraceIdLink,
  TrendStrip,
} from './components';
import { ApmFilters } from './Filters';
import { formatCount, formatDuration, formatRate, formatTimestamp } from './format';
import {
  hasActiveEntityFilters,
  servicePath,
  signalHref,
  transactionPath,
} from './model';
import { useApmFilters } from './useApmFilters';

export function ApmOverview() {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const { orgId, filters, params, setFilter, setFilters, clearFilters } = useApmFilters();
  const query = useQuery({
    queryKey: apmQueryKeys.overview(orgId, params),
    queryFn: () => apmApi.overview(params),
    enabled: Boolean(orgId),
    staleTime: 30_000,
  });
  const search = filters.search.trim().toLowerCase();
  const services = filterBySearch(
    query.data?.services ?? [],
    search,
    (row) => `${row.service.namespace} ${row.service.name} ${row.service.environment}`,
  );
  const dependencies = filterBySearch(
    query.data?.top_dependencies ?? [],
    search,
    (row) => `${row.dependency.category} ${row.dependency.target}`,
  );
  const errors = filterBySearch(
    query.data?.top_errors ?? [],
    search,
    (row) => `${row.error.error_type} ${row.representative_message ?? ''}`,
  );
  const transactions = filterBySearch(
    query.data?.top_transactions ?? [],
    search,
    (row) => `${row.transaction.name} ${row.transaction.kind} ${row.service.name}`,
  );
  const focusedService =
    filters.service && query.data?.services.length === 1
      ? query.data.services[0]
      : undefined;
  const serviceCount = query.data
    ? Object.values(query.data.service_health).reduce((sum, value) => sum + value, 0)
    : 0;
  const compactHealth = serviceCount <= 4;
  const empty = Boolean(query.data && query.data.red.request_count === 0 && services.length === 0);

  return (
    <ApmPageFrame
      title={t('overview.title')}
      subtitle={t('overview.subtitle')}
      meta={query.data?.meta}
      toolbar={
        <ApmFilters
          filters={filters}
          setFilter={setFilter}
          clearFilters={clearFilters}
        />
      }
    >
      <QueryBoundary
        pending={query.isPending}
        error={query.error}
        empty={empty}
        filtered={hasActiveEntityFilters(filters)}
        refetching={query.isFetching && Boolean(query.data)}
        onRetry={() => void query.refetch()}
      >
        {query.data && (
          <div className="space-y-5">
            <RedKpis
              red={query.data.red}
              trend={query.data.trend}
              resolution={query.data.meta.resolution}
            />
            {compactHealth && (
              <ServiceHealthPanel counts={query.data.service_health} compact />
            )}
            {compactHealth ? (
              <TrendStrip
                points={query.data.trend}
                range={query.data.meta.range}
                resolution={query.data.meta.resolution}
              />
            ) : (
              <div className="grid gap-5 xl:grid-cols-[minmax(0,2fr)_minmax(280px,1fr)]">
                <TrendStrip
                  points={query.data.trend}
                  range={query.data.meta.range}
                  resolution={query.data.meta.resolution}
                />
                <ServiceHealthPanel counts={query.data.service_health} />
              </div>
            )}
            {focusedService ? (
              <>
                <FocusedServiceContext service={focusedService} />
                <Section
                  title={t('overview.high_impact_transactions')}
                  description={t('overview.high_impact_transactions_description')}
                  action={
                    <SectionLink
                      to={scopedPath('/apm/transactions', focusedService)}
                      label={t('actions.view_all')}
                    />
                  }
                >
                  <DataTable
                    rows={transactions}
                    columns={transactionColumns(t)}
                    rowKey={(row) =>
                      `${row.version ?? ''}:${row.transaction.kind}:${row.transaction.name}`
                    }
                    onRowClick={(row) => navigate(transactionPath(row))}
                    emptyLabel={t('states.no_transactions')}
                  />
                </Section>
              </>
            ) : (
              <Section
                title={t('overview.high_impact_services')}
                description={t('overview.high_impact_services_description')}
                action={<SectionLink to="/apm/services" label={t('actions.view_all')} />}
              >
                <DataTable
                  rows={services}
                  columns={serviceColumns(t)}
                  rowKey={(row) =>
                    row.service.namespace + row.service.name + row.service.environment
                  }
                  onRowClick={(row) => navigate(servicePath(row.service))}
                  emptyLabel={t('states.no_services')}
                />
              </Section>
            )}
            <div className="grid gap-5 2xl:grid-cols-2">
              <Section
                title={t('overview.dependencies')}
                description={t('overview.dependencies_description')}
                action={
                  <SectionLink
                    to={
                      focusedService
                        ? scopedPath('/apm/dependencies', focusedService)
                        : '/apm/dependencies'
                    }
                    label={t('actions.explore')}
                  />
                }
              >
                <DataTable
                  rows={dependencies}
                  columns={dependencyColumns(t)}
                  rowKey={(row) => `${row.service.name}:${row.dependency.category}:${row.dependency.target}`}
                  emptyLabel={t('states.no_dependencies')}
                />
              </Section>
              <Section
                title={t('overview.top_errors')}
                description={t('overview.top_errors_description')}
                action={
                  <SectionLink
                    to={
                      focusedService
                        ? scopedPath('/apm/errors', focusedService)
                        : '/apm/errors'
                    }
                    label={t('actions.explore')}
                  />
                }
              >
                <DataTable
                  rows={errors}
                  columns={errorColumns(t)}
                  rowKey={(row) => row.error.fingerprint}
                  onRowClick={(row) =>
                    navigate(`/apm/errors/${encodeURIComponent(row.error.fingerprint)}`)
                  }
                  emptyLabel={t('states.no_errors')}
                />
              </Section>
            </div>
            {focusedService && (
              <TraceExemplars
                exemplars={query.data.red.exemplars}
                tracesHref={signalHref('traces', focusedService.traces, {
                  traceSort: 'duration_desc',
                })}
              />
            )}
            <Section title={t('overview.recent_versions')}>
              <div className="flex flex-wrap gap-2 p-4">
                {query.data.recent_versions.length === 0 ? (
                  <span className="text-xs text-tx-3">{t('states.no_versions')}</span>
                ) : (
                  query.data.recent_versions.map((version) => (
                    <button
                      key={`${version.service.name}:${version.version}`}
                      type="button"
                      onClick={() =>
                        setFilters({
                          service: version.service.name,
                          version: version.version,
                        })
                      }
                      className="rounded-md border border-bd-0 bg-bg-2 px-2.5 py-1.5 text-left outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
                    >
                      <span className="block text-xs font-strong text-tx-0">
                        {version.service.name}
                      </span>
                      <span className="font-mono text-xs text-tx-2">
                        {version.version}
                      </span>
                    </button>
                  ))
                )}
              </div>
            </Section>
          </div>
        )}
      </QueryBoundary>
    </ApmPageFrame>
  );
}

function ServiceHealthPanel({
  counts,
  compact = false,
}: {
  counts: { healthy: number; warning: number; critical: number; no_traffic: number };
  compact?: boolean;
}) {
  const { t } = useTranslation('apm');
  const items = [
    ['healthy', counts.healthy, CheckCircle2, 'text-green'],
    ['warning', counts.warning, AlertTriangle, 'text-yellow'],
    ['critical', counts.critical, Activity, 'text-red'],
    ['no_traffic', counts.no_traffic, CircleOff, 'text-tx-3'],
  ] as const;
  if (compact) {
    return (
      <section className="flex flex-col gap-3 rounded-lg border border-bd-0 bg-bg-1 px-4 py-3 sm:flex-row sm:items-center">
        <h2 className="shrink-0 type-section-title font-strong text-tx-0">
          {t('health.title')}
        </h2>
        <div className="flex flex-wrap gap-x-4 gap-y-2 sm:ml-auto">
          {items.map(([key, value, Icon, tone]) => (
            <span key={key} className="inline-flex items-center gap-1.5 text-xs text-tx-2">
              <Icon aria-hidden className={`h-3.5 w-3.5 ${tone}`} />
              <span>{t(`health.${key}`)}</span>
              <strong className="font-strong tabular-nums text-tx-0">
                {formatCount(value)}
              </strong>
            </span>
          ))}
        </div>
      </section>
    );
  }
  return (
    <section className="rounded-lg border border-bd-0 bg-bg-1 p-4">
      <h2 className="type-section-title font-strong text-tx-0">{t('health.title')}</h2>
      <div className="mt-4 grid grid-cols-2 gap-px overflow-hidden rounded-md border border-bd-0 bg-bd-0">
        {items.map(([key, value, Icon, tone]) => (
          <div key={key} className="bg-bg-1 p-3">
            <Icon aria-hidden className={`h-4 w-4 ${tone}`} />
            <div className="mt-2 text-xl font-display-strong tabular-nums text-tx-0">
              {formatCount(value)}
            </div>
            <div className="mt-0.5 text-xs text-tx-2">{t(`health.${key}`)}</div>
          </div>
        ))}
      </div>
    </section>
  );
}

function FocusedServiceContext({ service }: { service: ServiceSummary }) {
  const { t } = useTranslation('apm');
  return (
    <div className="flex flex-col gap-3 rounded-lg border border-indigo/20 bg-indigo-dim px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
      <div className="min-w-0">
        <div className="text-xs font-strong text-indigo-soft">
          {t('overview.focused_service')}
        </div>
        <div className="mt-0.5 truncate text-sm font-strong text-tx-0">
          {service.service.name}
          <span className="ml-2 font-normal text-tx-2">
            {service.service.namespace} · {service.service.environment}
          </span>
        </div>
      </div>
      <SectionLink
        to={servicePath(service.service)}
        label={t('actions.open_service_workbench')}
      />
    </div>
  );
}

function TraceExemplars({
  exemplars,
  tracesHref,
}: {
  exemplars: TraceExemplar[];
  tracesHref: string;
}) {
  const { t } = useTranslation('apm');
  return (
    <Section
      title={t('overview.slowest_traces')}
      description={t('overview.slowest_traces_description')}
      action={<SectionLink to={tracesHref} label={t('actions.explore')} />}
    >
      {exemplars.length === 0 ? (
        <div className="px-4 py-6 text-sm text-tx-3">{t('states.no_traces')}</div>
      ) : (
        <div className="divide-y divide-bd-0">
          {exemplars.map((exemplar) => (
            <div
              key={`${exemplar.trace_id}:${exemplar.span_id}`}
              className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3 text-xs"
            >
              <TraceIdLink
                traceId={exemplar.trace_id}
                spanId={exemplar.span_id}
                className="flex-1"
              />
              <span className="tabular-nums text-tx-2">
                {formatTimestamp(exemplar.event_time)}
              </span>
              <span className="min-w-20 text-right font-strong tabular-nums text-tx-0">
                {formatDuration(exemplar.duration_micros)}
              </span>
              {exemplar.trace_available ? (
                <Link
                  to={`/traces/${encodeURIComponent(exemplar.trace_id)}`}
                  className="inline-flex min-h-8 items-center gap-1 rounded px-2 font-strong text-indigo-soft outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
                >
                  {t('actions.open_trace')}
                  <ExternalLink aria-hidden className="h-3 w-3" />
                </Link>
              ) : (
                <span className="text-tx-3">{t('exemplars.unavailable')}</span>
              )}
            </div>
          ))}
        </div>
      )}
    </Section>
  );
}

function serviceColumns(t: (key: string) => string): DataTableColumn<ServiceSummary>[] {
  return [
    {
      key: 'service',
      header: t('columns.service'),
      cell: (row) => (
        <span className="inline-flex items-center gap-2">
          <HealthDot status={row.health} />
          <span>
            <span className="block font-strong text-tx-0">{row.service.name}</span>
            <span className="text-xs text-tx-3">
              {row.service.namespace} · {row.service.environment}
            </span>
          </span>
        </span>
      ),
    },
    { key: 'requests', header: t('columns.requests'), cell: (row) => formatCount(row.red.request_count) },
    { key: 'errors', header: t('columns.error_rate'), cell: (row) => formatRate(row.red.error_rate) },
    { key: 'p95', header: t('columns.p95'), cell: (row) => formatDuration(row.red.p95_micros) },
  ];
}

function transactionColumns(
  t: (key: string) => string,
): DataTableColumn<TransactionSummary>[] {
  return [
    {
      key: 'transaction',
      header: t('columns.transaction'),
      cell: (row) => (
        <span>
          <span className="block font-strong text-tx-0">{row.transaction.name}</span>
          <span className="text-xs text-tx-3">
            {t(`transaction_kinds.${row.transaction.kind}`)}
          </span>
        </span>
      ),
    },
    {
      key: 'requests',
      header: t('columns.requests'),
      cell: (row) => formatCount(row.red.request_count),
    },
    {
      key: 'errors',
      header: t('columns.error_rate'),
      cell: (row) => formatRate(row.red.error_rate),
    },
    {
      key: 'p95',
      header: t('columns.p95'),
      cell: (row) => formatDuration(row.red.p95_micros),
    },
    {
      key: 'total',
      header: t('columns.total_time'),
      cell: (row) => formatDuration(row.total_time_micros),
    },
  ];
}

function dependencyColumns(t: (key: string) => string): DataTableColumn<DependencySummary>[] {
  return [
    { key: 'target', header: t('columns.target'), cell: (row) => row.dependency.target },
    { key: 'category', header: t('columns.category'), cell: (row) => t(`dependency_categories.${row.dependency.category}`) },
    { key: 'p95', header: t('columns.p95'), cell: (row) => formatDuration(row.red.p95_micros) },
  ];
}

function errorColumns(t: (key: string) => string): DataTableColumn<ErrorSummary>[] {
  return [
    { key: 'error', header: t('columns.error'), cell: (row) => row.error.error_type },
    { key: 'service', header: t('columns.service'), cell: (row) => row.service.name },
    { key: 'count', header: t('columns.occurrences'), cell: (row) => formatCount(row.occurrence_count) },
  ];
}

function filterBySearch<T>(rows: T[], search: string, text: (row: T) => string): T[] {
  if (!search) return rows;
  return rows.filter((row) => text(row).toLowerCase().includes(search));
}

function scopedPath(path: string, service: ServiceSummary): string {
  const params = new URLSearchParams({
    namespace: service.service.namespace,
    service: service.service.name,
    environment: service.service.environment,
  });
  return `${path}?${params}`;
}
