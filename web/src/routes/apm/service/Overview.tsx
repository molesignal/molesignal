import { useQuery } from '@tanstack/react-query';
import { ExternalLink, GitCompareArrows, RadioTower } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useParams } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import {
  apmApi,
  apmQueryKeys,
  type DependencySummary,
  type ErrorSummary,
  type RedSummary,
  type ServiceDetailResponse,
  type TransactionSummary,
} from '@/api/apm';

import {
  ApmPageFrame,
  QueryBoundary,
  RedKpis,
  Section,
  SectionLink,
  TraceIdLink,
  TrendStrip,
} from '../components';
import { ApmFilters } from '../Filters';
import { formatCount, formatDuration, formatRate, formatTimestamp } from '../format';
import { signalHref, transactionPath } from '../model';
import { useApmFilters } from '../useApmFilters';
import { ServiceNavigation } from './Navigation';

export function ApmServiceDetail() {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const { service = '' } = useParams();
  const { orgId, filters, params, setFilter, clearFilters } = useApmFilters();
  const query = useQuery({
    queryKey: apmQueryKeys.service(orgId, service, params),
    queryFn: () => apmApi.service(service, params),
    enabled: Boolean(orgId && service),
    staleTime: 30_000,
  });
  const detail = query.data;
  return (
    <ApmPageFrame
      title={detail?.service.service.name ?? service}
      subtitle={
        detail
          ? `${detail.service.service.namespace} · ${detail.service.service.environment}`
          : t('services.detail_subtitle')
      }
      meta={detail?.meta}
      navigation={
        detail ? (
          <ServiceNavigation
            active="overview"
            service={detail.service.service}
            traces={detail.service.traces}
            version={filters.version}
          />
        ) : null
      }
      toolbar={
        <ApmFilters
          filters={filters}
          setFilter={setFilter}
          clearFilters={clearFilters}
          showSearch={false}
          showService={false}
        />
      }
    >
      <QueryBoundary
        pending={query.isPending}
        error={query.error}
        empty={false}
        refetching={query.isFetching && Boolean(detail)}
        onRetry={() => void query.refetch()}
      >
        {detail && (
          <div className="space-y-5">
            <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-bd-0 bg-bg-1 px-4 py-3">
              <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-xs text-tx-2">
                <span>
                  {t('services.first_seen')}: {formatTimestamp(detail.service.first_seen_at)}
                </span>
                <span>
                  {t('services.instances')}: {formatCount(detail.service.instrumentation.recent_instance_count)}
                </span>
                <span className="inline-flex items-center gap-1.5">
                  <RadioTower aria-hidden className="h-3.5 w-3.5" />
                  {detail.service.instrumentation.telemetry_sdk_name ?? t('values.unknown')}
                </span>
              </div>
              <div className="flex flex-wrap gap-1">
                {(['traces', 'logs', 'metrics', 'profiles'] as const).map((target) => (
                  <Link
                    key={target}
                    to={signalHref(target, detail.service.traces)}
                    className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs font-strong text-indigo-soft outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
                  >
                    {t(`signals.${target}`)}
                    <ExternalLink aria-hidden className="h-3 w-3" />
                  </Link>
                ))}
              </div>
            </div>
            <RedKpis
              red={detail.red}
              trend={detail.trend}
              resolution={detail.meta.resolution}
            />
            <TrendStrip
              points={detail.trend}
              range={detail.meta.range}
              resolution={detail.meta.resolution}
            />
            <Exemplars red={detail.red} />
            <Section
              title={t('transactions.title')}
              description={t('services.transactions_description')}
              action={<SectionLink to={scopedPath('/apm/transactions', detail)} label={t('actions.view_all')} />}
            >
              <DataTable
                rows={detail.transactions.slice(0, 10)}
                columns={transactionColumns(t)}
                rowKey={(row) => `${row.version ?? ''}:${row.transaction.name}`}
                onRowClick={(row) => navigate(transactionPath(row))}
              />
            </Section>
            <Section
              title={t('dependencies.title')}
              description={t('services.dependencies_description')}
              action={<SectionLink to={scopedPath('/apm/dependencies', detail)} label={t('actions.view_all')} />}
            >
              <DataTable
                rows={detail.dependencies.slice(0, 10)}
                columns={dependencyColumns(t)}
                rowKey={(row) => `${row.version ?? ''}:${row.dependency.category}:${row.dependency.target}`}
              />
            </Section>
            <Section
              title={t('errors.title')}
              description={t('services.errors_description')}
              action={<SectionLink to={scopedPath('/apm/errors', detail)} label={t('actions.view_all')} />}
            >
              <DataTable
                rows={detail.errors.slice(0, 10)}
                columns={errorColumns(t)}
                rowKey={(row) => row.error.fingerprint}
                onRowClick={(row) =>
                  navigate(`/apm/errors/${encodeURIComponent(row.error.fingerprint)}`)
                }
              />
            </Section>
            <Section
              title={t('versions.title')}
              action={
                detail.versions.length >= 2 ? (
                  <Link
                    to={`${scopedPath('/apm/deployments', detail)}&baseline=${encodeURIComponent(detail.versions[1]?.version ?? '')}&candidate=${encodeURIComponent(detail.versions[0]?.version ?? '')}`}
                    className="inline-flex items-center gap-1.5 rounded px-2 py-1 text-xs font-strong text-indigo-soft outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
                  >
                    <GitCompareArrows aria-hidden className="h-3.5 w-3.5" />
                    {t('versions.compare')}
                  </Link>
                ) : undefined
              }
            >
              <div className="flex flex-wrap gap-2 p-4">
                {detail.versions.map((version) => (
                  <button
                    key={version.version}
                    type="button"
                    onClick={() => setFilter('version', version.version)}
                    className="rounded-md border border-bd-0 bg-bg-2 px-3 py-2 text-left outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
                  >
                    <span className="block font-mono text-xs font-strong text-tx-0">
                      {version.version}
                    </span>
                    <span className="mt-1 block text-xs text-tx-3">
                      {formatCount(version.observation_count)} {t('versions.observations')}
                    </span>
                  </button>
                ))}
              </div>
            </Section>
          </div>
        )}
      </QueryBoundary>
    </ApmPageFrame>
  );
}

function Exemplars({ red }: { red: RedSummary }) {
  const { t } = useTranslation('apm');
  if (red.exemplars.length === 0) return null;
  return (
    <Section title={t('exemplars.title')} description={t('exemplars.description')}>
      <div className="divide-y divide-bd-0">
        {red.exemplars.map((exemplar) => (
          <div key={`${exemplar.trace_id}:${exemplar.span_id}`} className="flex items-center gap-3 px-4 py-2.5 text-xs">
            <TraceIdLink
              traceId={exemplar.trace_id}
              spanId={exemplar.span_id}
            >
              {exemplar.trace_id.slice(0, 16)}…
            </TraceIdLink>
            <span className="ml-auto tabular-nums text-tx-2">{formatDuration(exemplar.duration_micros)}</span>
            {exemplar.trace_available ? (
              <Link
                to={`/traces/${encodeURIComponent(exemplar.trace_id)}`}
                className="rounded px-1.5 py-1 font-strong text-indigo-soft outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
              >
                {t('actions.open_trace')}
              </Link>
            ) : (
              <span className="text-tx-3">{t('exemplars.unavailable')}</span>
            )}
          </div>
        ))}
      </div>
    </Section>
  );
}

function scopedPath(path: string, detail: ServiceDetailResponse): string {
  const service = detail.service.service;
  const params = new URLSearchParams({
    service: service.name,
    namespace: service.namespace,
    environment: service.environment,
  });
  return `${path}?${params}`;
}

function transactionColumns(t: (key: string) => string): DataTableColumn<TransactionSummary>[] {
  return [
    { key: 'name', header: t('columns.transaction'), cell: (row) => row.transaction.name },
    { key: 'requests', header: t('columns.requests'), cell: (row) => formatCount(row.red.request_count) },
    { key: 'errors', header: t('columns.error_rate'), cell: (row) => formatRate(row.red.error_rate) },
    { key: 'p95', header: t('columns.p95'), cell: (row) => formatDuration(row.red.p95_micros) },
  ];
}

function dependencyColumns(t: (key: string) => string): DataTableColumn<DependencySummary>[] {
  return [
    { key: 'target', header: t('columns.target'), cell: (row) => row.dependency.target },
    { key: 'category', header: t('columns.category'), cell: (row) => t(`dependency_categories.${row.dependency.category}`) },
    { key: 'errors', header: t('columns.error_rate'), cell: (row) => formatRate(row.red.error_rate) },
    { key: 'p95', header: t('columns.p95'), cell: (row) => formatDuration(row.red.p95_micros) },
  ];
}

function errorColumns(t: (key: string) => string): DataTableColumn<ErrorSummary>[] {
  return [
    { key: 'type', header: t('columns.error'), cell: (row) => row.error.error_type },
    { key: 'message', header: t('columns.message'), cell: (row) => row.representative_message ?? '—' },
    { key: 'count', header: t('columns.occurrences'), cell: (row) => formatCount(row.occurrence_count) },
  ];
}
