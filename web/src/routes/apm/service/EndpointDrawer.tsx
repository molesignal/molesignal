import { useQuery } from '@tanstack/react-query';
import { Bug, ChartColumn, Tags } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { apmApi, apmQueryKeys, type TransactionIdentity } from '@/api/apm';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { useKeyTransactions } from './keyTransactions';
import { SlowRequests } from './SlowRequests';
import { TimeSpent } from './TimeSpent';
import type { ServiceAddress } from './useService';
import { LatencyDistribution } from '../charts/LatencyDistribution';
import { RedCharts } from '../charts/RedCharts';
import { StatusCodes } from '../charts/StatusCodes';
import { IssueBadge } from '../errors/IssueBadge';
import { formatCount, formatDuration, formatRate } from '../format';
import { AlertCount, incidentsAbout, NewApmAlert, useActiveIncidents } from '../frame/alerts';
import { ApmDrawer } from '../frame/ApmDrawer';
import { DrawerTrail } from '../frame/DrawerTrail';
import { InvestigateButton } from '../frame/investigate';
import { StarToggle } from '../frame/OpenRow';
import { ApmCard, ApmQueryState } from '../frame/Page';
import { useAddressedDrawer } from '../frame/useApmScope';
import { compareEnvironment, compareShiftMicros } from '../scope';

const KINDS: readonly TransactionIdentity['kind'][] = ['http', 'rpc', 'messaging', 'span', 'other'];

/**
 * An endpoint of the service opened over the page: its requests, errors and
 * latency, how its latencies spread, its slowest requests (found again in
 * the trace store, so each one opens), its errors and the versions it ran.
 */
export function EndpointDrawer({ address }: { address: ServiceAddress }) {
  const { t } = useTranslation('apm');
  const drawer = useAddressedDrawer('endpoint');
  const error = useAddressedDrawer('error');
  const name = drawer.value;
  const rawKind = drawer.searchParams.get('kind') as TransactionIdentity['kind'] | null;
  const kind = rawKind && KINDS.includes(rawKind) ? rawKind : 'http';
  const params = address.queryParams(address.range, { kind });
  const compareParams = address.compareParams({ kind });

  const detail = useQuery({
    queryKey: apmQueryKeys.transaction(address.orgId, name ?? '', params),
    queryFn: () => apmApi.transaction(name ?? '', params),
    enabled: Boolean(address.orgId && name),
  });
  const compared = useQuery({
    queryKey: apmQueryKeys.transaction(address.orgId, name ?? '', compareParams ?? {}),
    queryFn: () => apmApi.transaction(name ?? '', compareParams ?? {}),
    enabled: Boolean(address.orgId && name && compareParams),
  });

  const data = detail.data;
  const keyed = useKeyTransactions(address);
  const otherEnvironment = compareEnvironment(address.scope.compare);
  const comparedLabel = otherEnvironment ? t('charts.compared_env', { environment: otherEnvironment }) : undefined;
  const incidents = useActiveIncidents();
  const alerts = name ? incidentsAbout(incidents.data, { name: address.name, environment: address.environment }, name) : [];

  return (
    <ApmDrawer
      open={Boolean(name)}
      onClose={() => drawer.close(['kind'])}
      testId="endpoint"
      trail={<DrawerTrail over="endpoint" />}
      actions={
        name ? (
          <>
            {(keyed.canChange || keyed.isKey({ name, kind })) && (
              <StarToggle
                on={keyed.isKey({ name, kind })}
                label={keyed.title({ name, kind })}
                onToggle={() => keyed.toggle({ name, kind })}
                disabled={!keyed.canChange}
              />
            )}
            <AlertCount incidents={alerts} />
            <NewApmAlert service={address.identity} endpoint={{ name, kind }} />
            <InvestigateButton subject={{ service: address.identity, range: address.range, endpoint: name }} />
          </>
        ) : undefined
      }
      title={name ? <TraceOperationName operation={name} /> : ''}
      subtitle={`${address.name} · ${t(`endpoints.kind.${kind}`)}`}
    >
      <ApmQueryState query={detail} empty={false}>
        {data && name && (
          <>
            <RedCharts
              trend={data.trend}
              compared={compareParams ? compared.data?.trend : null}
              shiftMicros={compareShiftMicros(address.scope.compare)}
              resolution={data.meta.resolution}
              versions={data.versions}
              range={{ from: data.meta.range.from, to: data.meta.range.to }}
              cursorScope="apm-endpoint-drawer"
              comparedLabel={comparedLabel}
            />
            <div className="grid min-w-0 gap-3 xl:grid-cols-2">
              <ApmCard icon={ChartColumn} tone="text-[var(--chart-3)]" title={t('charts.distribution')} testId="endpoint-distribution">
                <LatencyDistribution
                  buckets={data.latencyDistribution ?? []}
                  compared={compareParams ? compared.data?.latencyDistribution : null}
                  red={data.transaction.red}
                  apdexThreshold={data.transaction.apdexThreshold}
                  comparedLabel={comparedLabel}
                />
              </ApmCard>
              <SlowRequests address={address} transaction={{ name, kind }} />
            </div>
            <div className="grid min-w-0 gap-3 xl:grid-cols-2">
              <StatusCodes
                red={data.transaction.red}
                trend={data.trend}
                resolution={data.meta.resolution}
                range={{ from: data.meta.range.from, to: data.meta.range.to }}
              />
              <TimeSpent address={address} transaction={{ name, kind }} />
            </div>
            <div className="grid min-w-0 gap-3 xl:grid-cols-2">
              <ApmCard icon={Bug} tone="text-[var(--red-ink)]" title={t('endpoint.errors')} testId="endpoint-errors">
                {data.errors.length === 0 ? (
                  <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('endpoint.no_errors')}</p>
                ) : (
                  data.errors.map((item) => (
                    <button
                      key={item.error.fingerprint}
                      type="button"
                      onClick={() => error.open(item.error.fingerprint)}
                      className="grid h-8 w-full grid-cols-[minmax(0,1fr)_auto_64px] items-center gap-3 border-b border-bd-0 px-3 text-left font-sans text-xs outline-none last:border-b-0 hover:bg-bg-2 focus-visible:bg-bg-2"
                    >
                      <span className="flex min-w-0 items-center gap-2">
                        <span className="shrink-0 font-strong text-[var(--red-ink)]">{item.error.errorType}</span>
                        <span className="truncate text-tx-2">{item.representativeMessage ?? ''}</span>
                      </span>
                      <IssueBadge issue={item.issue} />
                      <span className="text-right tabular-nums text-tx-1">{formatCount(item.occurrenceCount)}</span>
                    </button>
                  ))
                )}
              </ApmCard>
              <ApmCard icon={Tags} tone="text-[var(--purple-ink)]" title={t('endpoint.versions')} testId="endpoint-versions">
                {data.versions.length === 0 ? (
                  <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('endpoint.no_versions')}</p>
                ) : (
                  data.versions.map((version) => (
                    <div
                      key={version.version}
                      className="grid h-8 grid-cols-[minmax(0,1fr)_96px] items-center gap-3 border-b border-bd-0 px-3 font-sans text-xs last:border-b-0"
                    >
                      <span className="truncate text-tx-0">{version.version}</span>
                      <span className="text-right tabular-nums text-tx-2">{formatCount(version.observationCount)}</span>
                    </div>
                  ))
                )}
              </ApmCard>
            </div>
            <p className="font-sans text-xs text-tx-3">
              {t('endpoint.summary', {
                requests: formatCount(data.transaction.red.requestCount),
                rate: formatRate(data.transaction.red.errorRate),
                p95: formatDuration(data.transaction.red.p95Time),
              })}
            </p>
          </>
        )}
      </ApmQueryState>
    </ApmDrawer>
  );
}
