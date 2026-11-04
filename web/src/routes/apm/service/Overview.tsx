import { Bug, ChartColumn, Route, Star, Workflow } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { DependencySummary, ServiceDetailResponse, TransactionSummary } from '@/api/apm';
import { cn } from '@/shell/lib/cn';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { useKeyTransactions } from './keyTransactions';
import type { ServiceAddress } from './useService';
import { LatencyDistribution } from '../charts/LatencyDistribution';
import { RedCharts } from '../charts/RedCharts';
import { IssueBadge } from '../errors/IssueBadge';
import { formatCount, formatDuration, formatRate } from '../format';
import { ApmCard } from '../frame/Page';
import { useAddressedDrawer } from '../frame/useApmScope';
import { InsightsCard } from '../insights/InsightsCard';
import { useInsights } from '../insights/useInsights';
import { compareEnvironment, compareShiftMicros, OTHER_DIMENSION, servicePath } from '../scope';
import { ServiceSlos } from '../slo/ServiceSlos';

/**
 * A service at a glance: requests, errors and latency over the time in
 * view, how its latencies spread, where its time goes downstream, and the
 * endpoints and errors that weigh most.
 */
export function ServiceOverview({
  address,
  detail,
  compared,
}: {
  address: ServiceAddress;
  detail: ServiceDetailResponse;
  compared: ServiceDetailResponse | undefined;
}) {
  const { t } = useTranslation('apm');
  const endpoint = useAddressedDrawer('endpoint');
  const error = useAddressedDrawer('error');
  const keyed = useKeyTransactions(address);
  const otherEnvironment = compareEnvironment(address.scope.compare);
  const comparedLabel = otherEnvironment ? t('charts.compared_env', { environment: otherEnvironment }) : undefined;
  const range = { from: detail.meta.range.from, to: detail.meta.range.to };
  const endpoints = [...detail.transactions].sort((a, b) => b.totalTime - a.totalTime).slice(0, 6);
  const downstream = [...detail.dependencies].sort((a, b) => b.totalTime - a.totalTime).slice(0, 6);
  const errors = [...detail.errors].sort((a, b) => b.occurrenceCount - a.occurrenceCount).slice(0, 5);
  const insights = useInsights(address.orgId, address.queryParams(address.range));

  return (
    <div data-apm-service-overview="" className="space-y-[12px]">
      <RedCharts
        trend={detail.trend}
        compared={compared?.trend}
        shiftMicros={compareShiftMicros(address.scope.compare)}
        resolution={detail.meta.resolution}
        versions={detail.versions}
        range={range}
        comparedLabel={comparedLabel}
      />
      {insights.data && (
        // Where the time goes downstream and to which endpoints, the cards below say.
        <InsightsCard data={insights.data} range={address.range} service={address.identity} hide={['slow_dependency', 'hot_endpoint']} />
      )}
      <ServiceSlos address={address} />
      <div className="grid min-w-0 gap-[12px] lg:grid-cols-2">
        <ApmCard icon={ChartColumn} tone="text-[var(--chart-3)]" title={t('charts.distribution')} testId="distribution">
          <LatencyDistribution
            buckets={detail.latencyDistribution ?? []}
            compared={compared?.latencyDistribution}
            red={detail.red}
            apdexThreshold={detail.service.apdexThreshold}
            comparedLabel={comparedLabel}
          />
        </ApmCard>
        <ApmCard
          icon={Workflow}
          tone="text-[var(--chart-2)]"
          title={t('overview.downstream')}
          description={t('overview.downstream_description')}
          testId="downstream"
          actions={<SeeAll to={servicePath(address.identity, 'dependencies', address.searchParams)} />}
        >
          <ShareList
            items={downstream.map((item) => ({
              key: dependencyKey(item),
              label: <DependencyLabel item={item} />,
              detail: t(`dependencies.category.${item.dependency.category}`),
              value: item.totalTime,
            }))}
            empty={t('overview.no_downstream')}
          />
        </ApmCard>
      </div>
      <div className="grid min-w-0 gap-[12px] lg:grid-cols-2">
        <ApmCard
          icon={Route}
          tone="text-[var(--blue-ink)]"
          title={t('overview.top_endpoints')}
          testId="top-endpoints"
          actions={<SeeAll to={servicePath(address.identity, 'endpoints', address.searchParams)} />}
        >
          {endpoints.length === 0 ? (
            <Empty>{t('overview.no_endpoints')}</Empty>
          ) : (
            <>
              <div className="grid h-7 grid-cols-[48px_minmax(0,1fr)_72px_72px_72px] items-center gap-3 bg-[var(--control-surface)] px-3 font-sans text-xs font-strong text-tx-2">
                <span>{t('endpoints.columns.kind')}</span>
                <span>{t('endpoints.columns.endpoint')}</span>
                <span className="text-right">{t('endpoints.columns.requests')}</span>
                <span className="text-right">{t('endpoints.columns.error_rate')}</span>
                <span className="text-right">{t('endpoints.columns.p95')}</span>
              </div>
              {endpoints.map((item) => (
                <button
                  key={`${item.transaction.kind}:${item.transaction.name}`}
                  type="button"
                  onClick={() =>
                    endpoint.open(item.transaction.name, {
                      kind: item.transaction.kind,
                    })
                  }
                  className="grid h-8 w-full grid-cols-[48px_minmax(0,1fr)_72px_72px_72px] items-center gap-3 border-b border-bd-0 px-3 text-left font-sans text-xs outline-none last:border-b-0 hover:bg-bg-2 focus-visible:bg-bg-2"
                >
                  <EndpointKind item={item} />
                  <span className="flex min-w-0 items-center gap-1.5">
                    {keyed.isKey(item.transaction) && (
                      <Star role="img" aria-label={t('endpoints.key')} className="h-3 w-3 shrink-0 text-[var(--yellow-ink)]" fill="currentColor" />
                    )}
                    <EndpointName item={item} />
                  </span>
                  <span className="text-right tabular-nums text-tx-1">{formatCount(item.red.requestCount)}</span>
                  <span className={cn('text-right tabular-nums', item.red.errorCount > 0 ? 'text-[var(--red-ink)]' : 'text-tx-3')}>
                    {formatRate(item.red.errorRate)}
                  </span>
                  <span className="text-right tabular-nums text-tx-1">{formatDuration(item.red.p95Time)}</span>
                </button>
              ))}
            </>
          )}
        </ApmCard>
        <ApmCard icon={Bug} tone="text-[var(--red-ink)]" title={t('overview.top_errors')} testId="top-errors" actions={<SeeAll to={servicePath(address.identity, 'errors', address.searchParams)} />}>
          {errors.length === 0 ? (
            <Empty>{t('overview.no_errors')}</Empty>
          ) : (
            errors.map((item) => (
              <button
                key={item.error.fingerprint}
                type="button"
                onClick={() => error.open(item.error.fingerprint)}
                className="grid h-8 w-full grid-cols-[minmax(0,1fr)_auto_64px] items-center gap-3 border-b border-bd-0 px-3 text-left font-sans text-xs outline-none last:border-b-0 hover:bg-bg-2 focus-visible:bg-bg-2"
              >
                <span className="flex min-w-0 items-center gap-2">
                  <span className="shrink-0 font-strong text-[var(--red-ink)]">{item.error.errorType}</span>
                  <span className="truncate text-tx-2">{item.representativeMessage ?? item.error.applicationFrame ?? ''}</span>
                </span>
                <IssueBadge issue={item.issue} />
                <span className="text-right tabular-nums text-tx-1">{formatCount(item.occurrenceCount)}</span>
              </button>
            ))
          )}
        </ApmCard>
      </div>
    </div>
  );
}

/** Each kind of entry in an ink of its own, as the verbs of the names beside it are; the overflow row stays quiet. */
export const ENDPOINT_KIND_TONE: Record<TransactionSummary['transaction']['kind'], string> = {
  http: 'text-[var(--blue-ink)]',
  rpc: 'text-[var(--purple-ink)]',
  messaging: 'text-[var(--orange-ink)]',
  span: 'text-[var(--yellow-ink)]',
  other: 'text-tx-3',
};

/** What kind of entry an endpoint is (HTTP, RPC, MQ), as a column of its own. */
export function EndpointKind({ item }: { item: TransactionSummary }) {
  const { t } = useTranslation('apm');
  return (
    <span data-apm-endpoint-kind={item.transaction.kind} className={cn('truncate', ENDPOINT_KIND_TONE[item.transaction.kind])}>
      {t(`endpoints.kind.${item.transaction.kind}`)}
    </span>
  );
}

/** An endpoint by its name, the verb toned; the overflow row by what it stands for. */
export function EndpointName({ item }: { item: TransactionSummary }) {
  const { t } = useTranslation('apm');

  return (
    <span className="flex min-w-0 items-center">
      {item.transaction.name === OTHER_DIMENSION ? (
        <span className="truncate text-tx-3" title={t('overflow.other_hint')}>
          {t('overflow.other')}
        </span>
      ) : (
        <TraceOperationName mono={false} operation={item.transaction.name} className="min-w-0 text-tx-0" />
      )}
    </span>
  );
}

export function dependencyKey(item: DependencySummary): string {
  return `${item.dependency.category}:${item.dependency.target}:${item.dependency.operation ?? ''}`;
}

/** A dependency as its target, then the operation called on it with its verb toned. */
function DependencyLabel({ item }: { item: DependencySummary }) {
  const { t } = useTranslation('apm');
  const target = item.dependency.target === OTHER_DIMENSION ? t('overflow.other') : item.dependency.target;

  return (
    <span className="flex min-w-0 items-baseline gap-1.5 text-tx-0">
      <span className="min-w-0 shrink truncate" title={target}>
        {target}
      </span>
      {item.dependency.operation && (
        <>
          <span aria-hidden className="shrink-0 text-tx-3">
            ·
          </span>
          <TraceOperationName mono={false} operation={item.dependency.operation} className="min-w-0" />
        </>
      )}
    </span>
  );
}

/** Items as shares of their sum, the largest first, each with a bar. */
function ShareList({ items, empty }: { items: Array<{ key: string; label: React.ReactNode; detail: string; value: number }>; empty: string }) {
  const total = items.reduce((sum, item) => sum + item.value, 0);

  if (items.length === 0 || total === 0) return <Empty>{empty}</Empty>;

  return (
    <div>
      {items.map((item) => {
        const share = item.value / total;

        return (
          <div
            key={item.key}
            className="grid h-8 grid-cols-[minmax(0,1fr)_120px_64px] items-center gap-3 border-b border-bd-0 px-3 font-sans text-xs last:border-b-0"
          >
            <span className="flex min-w-0 items-center gap-2">
              <span className="min-w-0">{item.label}</span>
              <span className="shrink-0 text-tx-3">{item.detail}</span>
            </span>
            <span aria-hidden className="relative h-1.5 overflow-hidden rounded-full bg-bg-3">
              <span className="absolute inset-y-0 left-0 rounded-full bg-[var(--chart-1)]" style={{ width: `${Math.max(2, share * 100)}%` }} />
            </span>
            <span className="text-right tabular-nums text-tx-1">{formatRate(share)}</span>
          </div>
        );
      })}
    </div>
  );
}

function SeeAll({ to }: { to: string }) {
  const { t } = useTranslation('apm');
  return (
    <Link to={to} className="font-sans text-xs font-strong text-tx-2 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none">
      {t('actions.see_all')}
    </Link>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{children}</p>;
}
