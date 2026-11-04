import { useQuery } from '@tanstack/react-query';
import { Bird } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { apmApi, apmQueryKeys, type RedSummary } from '@/api/apm';
import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { formatRelativeMicros } from '@/time/relative';

import type { CanaryPair } from './deployTimeline';
import type { ServiceAddress } from './useService';
import { formatCount, formatDuration, formatRate } from '../format';
import { ApmCard, ApmQueryState } from '../frame/Page';
import { VERDICT_TEXT } from '../frame/VersionVerdict';
import { apmQueryParams } from '../scope';

const ROWS = [
  { key: 'requests', read: (red: RedSummary) => formatCount(red.requestCount) },
  { key: 'error_rate', read: (red: RedSummary) => formatRate(red.errorRate) },
  { key: 'p50', read: (red: RedSummary) => formatDuration(red.p50Time) },
  { key: 'p95', read: (red: RedSummary) => formatDuration(red.p95Time) },
  { key: 'p99', read: (red: RedSummary) => formatDuration(red.p99Time) },
] as const;

/**
 * Two versions running at once, as a canary runs beside the version it is
 * to replace: how the traffic is shared between them, and how each served,
 * side by side, since the newer one came.
 */
export function CanaryCard({ address, pair }: { address: ServiceAddress; pair: CanaryPair }) {
  const { t, i18n } = useTranslation('apm');
  const time = useTimeFormatter();
  const from = new Date(Math.max(pair.from / 1_000, address.range.from.getTime()));
  const params = {
    ...apmQueryParams(address.scope, { from, to: address.range.to }),
    service: address.name,
    ...(address.namespace ? { namespace: address.namespace } : {}),
    baseline: pair.baseline.version,
    candidate: pair.candidate.version,
  };
  const query = useQuery({
    queryKey: apmQueryKeys.compare(address.orgId, params),
    queryFn: () => apmApi.compareVersions(params),
    enabled: Boolean(address.orgId),
    refetchInterval: 60_000,
  });
  const compare = query.data;
  const total = compare ? compare.baseline.red.requestCount + compare.candidate.red.requestCount : 0;
  const share = compare && total > 0 ? compare.candidate.red.requestCount / total : null;

  return (
    <ApmCard
      icon={Bird}
      tone="text-[var(--yellow-ink)]"
      testId="canary"
      title={t('deployments.canary.title', { candidate: pair.candidate.version, baseline: pair.baseline.version })}
      description={
        <span title={time.micros(from.getTime() * 1_000, false)}>{t('deployments.canary.since', { since: formatRelativeMicros(from.getTime() * 1_000, i18n.language) })}</span>
      }
      actions={compare && <span className={cn('font-sans text-xs font-strong', VERDICT_TEXT[compare.status])}>{t(`deployments.status.${compare.status}`)}</span>}
    >
      <ApmQueryState query={query} empty={false} compact>
        {compare && (
          <div className="space-y-3 p-3 font-sans text-xs">
            {share !== null && (
              <div data-apm-canary-share={Math.round(share * 100)}>
                <div className="mb-1 flex items-center justify-between text-tx-2">
                  <span>
                    <span className="font-strong text-tx-0">{pair.candidate.version}</span> <span className="tabular-nums">{formatRate(share)}</span>
                  </span>
                  <span>
                    <span className="font-strong text-tx-0">{pair.baseline.version}</span> <span className="tabular-nums">{formatRate(1 - share)}</span>
                  </span>
                </div>
                <div className="flex h-2 overflow-hidden rounded-full bg-bg-3" role="img" aria-label={t('deployments.canary.share', { share: formatRate(share) })}>
                  <span className="h-full bg-[var(--yellow-ink)]" style={{ width: `${share * 100}%` }} />
                  <span className="h-full flex-1 bg-[var(--chart-1)] opacity-60" />
                </div>
              </div>
            )}
            <div role="table" aria-label={t('deployments.canary.figures')}>
              <div role="row" className="grid h-7 grid-cols-[minmax(0,1fr)_120px_120px] items-center gap-3 bg-[var(--table-head-surface)] px-2 font-strong text-tx-0">
                <span role="columnheader" />
                <span role="columnheader" className="truncate text-right" title={pair.candidate.version}>
                  {pair.candidate.version}
                </span>
                <span role="columnheader" className="truncate text-right" title={pair.baseline.version}>
                  {pair.baseline.version}
                </span>
              </div>
              {ROWS.map((row) => (
                <div key={row.key} role="row" className="grid h-7 grid-cols-[minmax(0,1fr)_120px_120px] items-center gap-3 border-t border-bd-0 px-2">
                  <span role="rowheader" className="text-tx-2">
                    {t(`deployments.canary.rows.${row.key}`)}
                  </span>
                  <span className="text-right tabular-nums text-tx-0">{row.read(compare.candidate.red)}</span>
                  <span className="text-right tabular-nums text-tx-1">{row.read(compare.baseline.red)}</span>
                </div>
              ))}
            </div>
            {!compare.sufficientData && <p className="text-tx-3">{t('deployments.insufficient')}</p>}
          </div>
        )}
      </ApmQueryState>
    </ApmCard>
  );
}
