import { useQuery } from '@tanstack/react-query';
import { ExternalLink, GitCompareArrows } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import {
  apmApi,
  apmQueryKeys,
  type ErrorSummary,
  type SignalFilterHandle,
  type TraceExemplar,
  type TransactionIdentity,
  type TransactionSummary,
  type VersionSummary,
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
import {
  formatCount,
  formatDuration,
  formatTimestamp,
} from '../format';
import { signalHref } from '../model';
import { ServiceNavigation } from '../service/Navigation';
import { useApmFilters } from '../useApmFilters';

export function ApmTransactionDetail() {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const { transaction = '' } = useParams();
  const [searchParams] = useSearchParams();
  const kind = transactionKind(searchParams.get('kind'));
  const { orgId, filters, params, setFilter, clearFilters } = useApmFilters();
  const queryParams = kind ? { ...params, kind } : params;
  const query = useQuery({
    queryKey: apmQueryKeys.transaction(orgId, transaction, queryParams),
    queryFn: () => apmApi.transaction(transaction, queryParams),
    enabled: Boolean(orgId && transaction),
    staleTime: 30_000,
  });
  const detail = query.data;
  const navigationVersion = filters.version || detail?.transaction.version;

  return (
    <ApmPageFrame
      title={detail?.transaction.transaction.name ?? transaction}
      subtitle={t('transactions.detail_subtitle')}
      meta={detail?.meta}
      navigation={
        detail ? (
          <ServiceNavigation
            active="transactions"
            service={detail.transaction.service}
            traces={detail.transaction.traces}
            {...(navigationVersion ? { version: navigationVersion } : {})}
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
            <TransactionContext detail={detail.transaction} />
            <SignalLinks traces={detail.transaction.traces} />
            <RedKpis
              red={detail.transaction.red}
              trend={detail.trend}
              resolution={detail.meta.resolution}
            />
            <TrendStrip
              points={detail.trend}
              range={detail.meta.range}
              resolution={detail.meta.resolution}
            />
            <TraceExemplars
              exemplars={detail.transaction.red.exemplars}
              tracesHref={signalHref('traces', detail.transaction.traces, {
                traceSort: 'duration_desc',
              })}
            />
            <Section
              title={t('transactions.errors')}
              description={t('transactions.errors_description')}
              action={
                <SectionLink
                  to={signalHref('logs', detail.transaction.traces)}
                  label={t('actions.related_logs')}
                />
              }
            >
              <DataTable
                rows={detail.errors}
                columns={errorColumns(t)}
                rowKey={(row) => row.error.fingerprint}
                onRowClick={(row) => navigate(errorPath(row))}
                emptyLabel={t('states.no_errors')}
              />
            </Section>
            <TransactionVersions
              versions={detail.versions}
              service={detail.transaction.service}
              onSelect={(version) => setFilter('version', version)}
            />
          </div>
        )}
      </QueryBoundary>
    </ApmPageFrame>
  );
}

function TransactionContext({
  detail,
}: {
  detail: TransactionSummary;
}) {
  const { t } = useTranslation('apm');
  const facts = [
    {
      label: t('columns.kind'),
      value: t(`transaction_kinds.${detail.transaction.kind}`),
    },
    { label: t('columns.service'), value: detail.service.name },
    { label: t('columns.version'), value: detail.version ?? '—' },
    {
      label: t('columns.total_time'),
      value: formatDuration(detail.total_time_micros),
    },
  ];
  return (
    <div className="grid gap-px overflow-hidden rounded-lg border border-bd-0 bg-bd-0 sm:grid-cols-2 xl:grid-cols-4">
      {facts.map((fact) => (
        <div key={fact.label} className="bg-bg-1 p-4">
          <span className="block text-xs text-tx-3">{fact.label}</span>
          <span className="mt-1 block truncate text-sm font-strong text-tx-0">
            {fact.value}
          </span>
        </div>
      ))}
    </div>
  );
}

function SignalLinks({
  traces,
}: {
  traces: SignalFilterHandle;
}) {
  const { t } = useTranslation('apm');
  return (
    <div className="flex flex-wrap gap-2">
      {(['traces', 'logs', 'metrics', 'profiles'] as const).map((target) => (
        <Link
          key={target}
          to={signalHref(
            target,
            traces,
            target === 'traces' ? { traceSort: 'duration_desc' } : {},
          )}
          className="inline-flex h-8 items-center gap-1.5 rounded-md border border-bd-0 bg-bg-1 px-3 text-xs font-strong text-indigo-soft outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
        >
          {t(`signals.${target}`)}
          <ExternalLink aria-hidden className="h-3 w-3" />
        </Link>
      ))}
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
      action={<SectionLink to={tracesHref} label={t('actions.filtered_traces')} />}
    >
      {exemplars.length === 0 ? (
        <div className="px-4 py-6 text-sm text-tx-3">
          {t('states.no_traces')}
        </div>
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

function TransactionVersions({
  versions,
  service,
  onSelect,
}: {
  versions: VersionSummary[];
  service: VersionSummary['service'];
  onSelect: (version: string) => void;
}) {
  const { t } = useTranslation('apm');
  const comparePath = deploymentPath(service, versions);
  return (
    <Section
      title={t('transactions.deployments')}
      description={t('transactions.deployments_description')}
      action={
        comparePath ? (
          <Link
            to={comparePath}
            className="inline-flex items-center gap-1.5 rounded px-2 py-1 text-xs font-strong text-indigo-soft outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
          >
            <GitCompareArrows aria-hidden className="h-3.5 w-3.5" />
            {t('versions.compare')}
          </Link>
        ) : undefined
      }
    >
      <div className="flex flex-wrap gap-2 p-4">
        {versions.length === 0 ? (
          <span className="text-xs text-tx-3">{t('states.no_versions')}</span>
        ) : (
          versions.map((version) => (
            <button
              key={version.version}
              type="button"
              onClick={() => onSelect(version.version)}
              className="rounded-md border border-bd-0 bg-bg-2 px-3 py-2 text-left outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
            >
              <span className="block font-mono text-xs font-strong text-tx-0">
                {version.version}
              </span>
              <span className="mt-1 block text-xs text-tx-3">
                {formatCount(version.observation_count)}{' '}
                {t('versions.observations')}
              </span>
            </button>
          ))
        )}
      </div>
    </Section>
  );
}

function errorColumns(
  t: (key: string) => string,
): DataTableColumn<ErrorSummary>[] {
  return [
    {
      key: 'error',
      header: t('columns.error'),
      cell: (row) => row.error.error_type,
    },
    {
      key: 'message',
      header: t('columns.message'),
      cell: (row) => row.representative_message ?? '—',
    },
    {
      key: 'count',
      header: t('columns.occurrences'),
      cell: (row) => formatCount(row.occurrence_count),
    },
    {
      key: 'last',
      header: t('columns.last_seen'),
      cell: (row) => formatTimestamp(row.last_seen_at),
    },
  ];
}

function transactionKind(
  value: string | null,
): TransactionIdentity['kind'] | undefined {
  return (
    ['http', 'rpc', 'messaging', 'span', 'other'] as const
  ).find((kind) => kind === value);
}

function errorPath(error: ErrorSummary): string {
  const params = new URLSearchParams({
    namespace: error.service.namespace,
    service: error.service.name,
    environment: error.service.environment,
  });
  return `/apm/errors/${encodeURIComponent(error.error.fingerprint)}?${params}`;
}

function deploymentPath(
  service: VersionSummary['service'],
  versions: VersionSummary[],
): string | undefined {
  if (versions.length < 2) return undefined;
  const params = new URLSearchParams({
    namespace: service.namespace,
    service: service.name,
    environment: service.environment,
    baseline: versions[1]?.version ?? '',
    candidate: versions[0]?.version ?? '',
  });
  return `/apm/deployments?${params}`;
}
