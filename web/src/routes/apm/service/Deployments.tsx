import { useQuery } from '@tanstack/react-query';
import { GitCompareArrows } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { apmApi, apmQueryKeys, type ServiceDetailResponse, type VersionCompareResponse } from '@/api/apm';
import { cn } from '@/shell/lib/cn';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { CanaryCard } from './CanaryCard';
import { canaryPair } from './deployTimeline';
import { DeployTimelineCard } from './DeployTimelineCard';
import type { ServiceAddress } from './useService';
import { formatCount, formatDuration, formatRelative, formatSigned } from '../format';
import { ApmCard, ApmQueryState } from '../frame/Page';
import { useAddressedDrawer } from '../frame/useApmScope';
import { VERDICT_TEXT } from '../frame/VersionVerdict';
import { apmQueryParams } from '../scope';

/** The most versions offered to compare. */
const MOST_VERSIONS = 10;

/**
 * The service's deployments, as Datadog's deployment tracking has them: two
 * versions running at once side by side, as a canary runs; the timeline of
 * what pipelines reported and spans carried, each version against the one
 * before it; then two versions compared, with the endpoints and errors that
 * got worse. A verdict opens its two versions in the comparison.
 */
export function ServiceDeployments({ address, detail }: { address: ServiceAddress; detail: ServiceDetailResponse }) {
  const versions = React.useMemo(() => [...detail.versions].sort((a, b) => b.firstSeenAt - a.firstSeenAt).slice(0, MOST_VERSIONS), [detail.versions]);
  const canary = React.useMemo(() => canaryPair(detail.versions), [detail.versions]);
  const [baseline, setBaseline] = React.useState(versions[1]?.version ?? '');
  const [candidate, setCandidate] = React.useState(versions[0]?.version ?? '');
  const comparison = React.useRef<HTMLDivElement>(null);

  const comparePair = (previous: string, version: string) => {
    setBaseline(previous);
    setCandidate(version);
    comparison.current?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  };

  return (
    <div className="space-y-[12px]">
      {canary && <CanaryCard address={address} pair={canary} />}
      <DeployTimelineCard address={address} versions={detail.versions} onCompare={comparePair} />
      {versions.length >= 2 && (
        <div ref={comparison} className="scroll-mt-3">
          <Comparison
            address={address}
            versions={versions.map((version) => version.version)}
            baseline={baseline}
            candidate={candidate}
            onBaseline={setBaseline}
            onCandidate={setCandidate}
          />
        </div>
      )}
    </div>
  );
}

function Comparison({
  address,
  versions,
  baseline,
  candidate,
  onBaseline,
  onCandidate,
}: {
  address: ServiceAddress;
  versions: string[];
  baseline: string;
  candidate: string;
  onBaseline: (value: string) => void;
  onCandidate: (value: string) => void;
}) {
  const { t } = useTranslation('apm');
  const endpoint = useAddressedDrawer('endpoint');
  const error = useAddressedDrawer('error');
  // Both versions are of the whole service: the version chosen for the page does not narrow them.
  const params = {
    ...apmQueryParams(address.scope, address.range),
    service: address.name,
    ...(address.namespace ? { namespace: address.namespace } : {}),
    baseline,
    candidate,
  };
  const query = useQuery({
    queryKey: apmQueryKeys.compare(address.orgId, params),
    queryFn: () => apmApi.compareVersions(params),
    enabled: Boolean(address.orgId && baseline && candidate && baseline !== candidate),
  });
  // A verdict can pick a version older than the ones listed.
  const options = [...new Set([...versions, baseline, candidate].filter(Boolean))].map((value) => ({ value, label: value }));
  const control = 'h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 hover:bg-bg-3';

  return (
    <ApmCard
      icon={GitCompareArrows}
      tone="text-[var(--purple-ink)]"
      testId="compare"
      title={t('deployments.compare')}
      actions={
        <span className="flex items-center gap-2 font-sans text-xs text-tx-2">
          <FittedSelect
            aria-label={t('deployments.baseline')}
            value={baseline}
            onValueChange={onBaseline}
            options={options}
            maxWidth={180}
            prefix={t('deployments.baseline')}
            className={control}
          />
          <span aria-hidden>→</span>
          <FittedSelect
            aria-label={t('deployments.candidate')}
            value={candidate}
            onValueChange={onCandidate}
            options={options}
            maxWidth={180}
            prefix={t('deployments.candidate')}
            className={control}
          />
        </span>
      }
    >
      <ApmQueryState query={query} empty={false} compact>
        {query.data && (
          <div className="space-y-3 p-3 font-sans text-xs">
            <Verdict compare={query.data} />
            <div className="grid gap-3 lg:grid-cols-2">
              <div>
                <h3 className="mb-1 font-strong text-tx-1">{t('deployments.regressed_endpoints')}</h3>
                {query.data.regressedTransactions.length === 0 ? (
                  <p className="text-tx-3">{t('deployments.none_regressed')}</p>
                ) : (
                  query.data.regressedTransactions.map((item) => (
                    <button
                      key={item.transaction.name}
                      type="button"
                      onClick={() =>
                        endpoint.open(item.transaction.name, {
                          kind: item.transaction.kind,
                        })
                      }
                      className="flex h-7 w-full items-center gap-2 rounded px-1 text-left hover:bg-bg-2"
                    >
                      <TraceOperationName mono={false} operation={item.transaction.name} className="min-w-0 text-tx-0" />
                      <span className="ml-auto tabular-nums text-tx-2">{formatDuration(item.red.p95Time)}</span>
                    </button>
                  ))
                )}
              </div>
              <div>
                <h3 className="mb-1 font-strong text-tx-1">{t('deployments.regressed_errors')}</h3>
                {query.data.regressedErrors.length === 0 ? (
                  <p className="text-tx-3">{t('deployments.none_regressed')}</p>
                ) : (
                  query.data.regressedErrors.map((item) => (
                    <button
                      key={item.error.fingerprint}
                      type="button"
                      onClick={() => error.open(item.error.fingerprint)}
                      className="flex h-7 w-full items-center gap-2 rounded px-1 text-left hover:bg-bg-2"
                    >
                      <span className="shrink-0 font-strong text-[var(--red-ink)]">{item.error.errorType}</span>
                      <span className="truncate text-tx-2">{item.representativeMessage ?? ''}</span>
                      <span className="ml-auto tabular-nums text-tx-2">{formatCount(item.occurrenceCount)}</span>
                    </button>
                  ))
                )}
              </div>
            </div>
          </div>
        )}
      </ApmQueryState>
    </ApmCard>
  );
}

function Verdict({ compare }: { compare: VersionCompareResponse }) {
  const { t } = useTranslation('apm');

  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
      <span className={cn('font-strong', VERDICT_TEXT[compare.status])}>{t(`deployments.status.${compare.status}`)}</span>
      <span>
        {t('deployments.delta.requests')} <span className="tabular-nums text-tx-0">{formatRelative(compare.delta.requestCountRelative)}</span>
      </span>
      <span>
        {t('deployments.delta.error_rate')} <span className="tabular-nums text-tx-0">{formatSigned(compare.delta.errorRateAbsolute, true)}</span>
      </span>
      <span>
        {t('deployments.delta.p95')} <span className="tabular-nums text-tx-0">{formatSigned(compare.delta.p95AbsoluteTime, false, true)}</span>
      </span>
      {!compare.sufficientData && <span className="text-tx-3">{t('deployments.insufficient')}</span>}
    </div>
  );
}
