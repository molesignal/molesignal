import { useQuery } from '@tanstack/react-query';
import { ExternalLink, FileWarning } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useParams } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import { apmApi, apmQueryKeys, type TransactionSummary } from '@/api/apm';

import {
  ApmPageFrame,
  QueryBoundary,
  Section,
  TrendStrip,
} from './components';
import { ApmFilters } from './Filters';
import { formatCount, formatDuration, formatTimestamp } from './format';
import { signalHref, transactionPath } from './model';
import { useApmFilters } from './useApmFilters';

export function ApmErrorDetail() {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const { fingerprint = '' } = useParams();
  const { orgId, filters, params, setFilter, clearFilters } = useApmFilters();
  const query = useQuery({
    queryKey: apmQueryKeys.error(orgId, fingerprint, params),
    queryFn: () => apmApi.error(fingerprint, params),
    enabled: Boolean(orgId && fingerprint),
    staleTime: 30_000,
  });
  const detail = query.data;

  return (
    <ApmPageFrame
      title={detail?.group.error.error_type ?? t('errors.detail_title')}
      subtitle={detail?.group.representative_message ?? t('errors.detail_subtitle')}
      meta={detail?.meta}
      toolbar={
        <ApmFilters
          filters={filters}
          setFilter={setFilter}
          clearFilters={clearFilters}
          showSearch={false}
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
            <div className="grid gap-px overflow-hidden rounded-lg border border-bd-0 bg-bd-0 sm:grid-cols-2 xl:grid-cols-4">
              <Fact label={t('columns.service')} value={detail.group.service.name} />
              <Fact
                label={t('columns.occurrences')}
                value={formatCount(detail.group.occurrence_count)}
              />
              <Fact
                label={t('errors.first_seen')}
                value={formatTimestamp(detail.group.first_seen_at)}
              />
              <Fact
                label={t('columns.last_seen')}
                value={formatTimestamp(detail.group.last_seen_at)}
              />
            </div>
            <div className="flex flex-wrap gap-2">
              <CrossSignalLink
                to={signalHref('logs', detail.group.traces)}
                label={t('actions.related_logs')}
              />
              <CrossSignalLink
                to={signalHref('traces', detail.group.traces, {
                  traceSort: 'errors_desc',
                })}
                label={t('actions.filtered_traces')}
              />
            </div>
            <TrendStrip
              points={detail.trend}
              range={detail.meta.range}
              resolution={detail.meta.resolution}
            />
            <Section
              title={t('errors.representative_stack')}
              description={t('errors.sanitized_stack_description')}
            >
              <Stack frames={detail.representative_stack} />
            </Section>
            <Section title={t('errors.affected_transactions')}>
              <DataTable
                rows={detail.affected_transactions}
                columns={transactionColumns(t)}
                rowKey={(row) => `${row.service.name}:${row.transaction.name}`}
                onRowClick={(row) => navigate(transactionPath(row))}
                emptyLabel={t('states.no_transactions')}
              />
            </Section>
            <Section title={t('errors.samples')}>
              <div className="divide-y divide-bd-0">
                {detail.samples.map((sample) => (
                  <div
                    key={`${sample.trace_id}:${sample.span_id}:${sample.event_time}`}
                    className="grid gap-2 px-4 py-3 text-xs md:grid-cols-[160px_minmax(0,1fr)_180px]"
                  >
                    <span className="text-tx-2">
                      {formatTimestamp(sample.event_time)}
                    </span>
                    <span className="truncate text-tx-1">
                      {sample.representative_message ?? t('values.no_message')}
                    </span>
                    {sample.trace_available && sample.trace_id ? (
                      <Link
                        to={`/traces/${encodeURIComponent(sample.trace_id)}`}
                        className="inline-flex items-center justify-self-start gap-1 rounded px-1.5 py-1 font-strong text-indigo-soft outline-none hover:bg-bg-2 focus-visible:bg-bg-2 md:justify-self-end"
                      >
                        {t('actions.open_trace')}
                        <ExternalLink aria-hidden className="h-3 w-3" />
                      </Link>
                    ) : (
                      <span className="text-tx-3 md:text-right">
                        {t('exemplars.unavailable')}
                      </span>
                    )}
                  </div>
                ))}
                {detail.samples.length === 0 && (
                  <div className="grid h-24 place-items-center text-xs text-tx-3">
                    {t('errors.no_samples')}
                  </div>
                )}
              </div>
            </Section>
          </div>
        )}
      </QueryBoundary>
    </ApmPageFrame>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="bg-bg-1 p-4">
      <span className="block text-xs text-tx-3">{label}</span>
      <span className="mt-1 block truncate text-sm font-strong text-tx-0">{value}</span>
    </div>
  );
}

function CrossSignalLink({ to, label }: { to: string; label: string }) {
  return (
    <Link
      to={to}
      className="inline-flex h-8 items-center gap-1.5 rounded-md border border-bd-0 bg-bg-1 px-3 text-xs font-strong text-indigo-soft outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
    >
      {label}
      <ExternalLink aria-hidden className="h-3 w-3" />
    </Link>
  );
}

function Stack({ frames }: { frames: string[] }) {
  const { t } = useTranslation('apm');
  if (frames.length === 0) {
    return (
      <div className="flex h-24 items-center justify-center gap-2 text-xs text-tx-3">
        <FileWarning aria-hidden className="h-4 w-4" />
        {t('errors.no_stack')}
      </div>
    );
  }
  return (
    <pre className="m-0 overflow-x-auto bg-bg-0 p-4 font-mono text-xs leading-6 text-tx-1">
      {frames.join('\n')}
    </pre>
  );
}

function transactionColumns(
  t: (key: string) => string,
): DataTableColumn<TransactionSummary>[] {
  return [
    {
      key: 'transaction',
      header: t('columns.transaction'),
      cell: (row) => row.transaction.name,
    },
    {
      key: 'kind',
      header: t('columns.kind'),
      cell: (row) => row.transaction.kind,
    },
    {
      key: 'requests',
      header: t('columns.requests'),
      cell: (row) => formatCount(row.red.request_count),
    },
    {
      key: 'p95',
      header: t('columns.p95'),
      cell: (row) => formatDuration(row.red.p95_micros),
    },
  ];
}
