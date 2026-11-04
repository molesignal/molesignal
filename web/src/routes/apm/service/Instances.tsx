import type { UseQueryResult } from '@tanstack/react-query';
import { Boxes } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ServiceInstance } from '@/api/apm';
import { cn } from '@/shell/lib/cn';
import { LONG_LIST_ROW_CLASS, longListRowStyle } from '@/shell/list/longList';
import { Sparkline } from '@/shell/readout/Sparkline';
import { formatRelativeMicros } from '@/time/relative';

import { signalHref } from './links';
import type { ServiceAddress } from './useService';
import { throughputOf } from '../catalog/model';
import { formatDuration, formatRate, formatThroughput } from '../format';
import { gridMinWidth } from '../frame/gridWidth';
import { OpenRow } from '../frame/OpenRow';
import { ApmCard, ApmQueryState } from '../frame/Page';
import type { RuntimeInstance } from '../runtime/metrics';
import { OTHER_DIMENSION } from '../scope';

const TRACKS = ['minmax(200px,1.4fr)', '96px', 'minmax(160px,1fr)', 'minmax(112px,0.7fr)', '152px', '72px', '72px', '96px', '96px'];
const ROW_STYLE = { height: 32, ...longListRowStyle(31) };
const COLUMNS = ['instance', 'version', 'where', 'runtime', 'throughput', 'error_rate', 'p95', 'last_seen', 'signals'] as const;
const NUMERIC = new Set<string>(['throughput', 'error_rate', 'p95', 'last_seen']);

/**
 * The instances that ran the service within the range, of the version in
 * view when one is: each with the version it last ran, where it runs (pod or
 * host, region) and its runtime, what it served, and its logs and metrics,
 * the most recently seen first. During a rollout the versions sit side by
 * side here. A line keeps the runtime charts to that instance.
 */
export function ServiceInstances({
  address,
  query,
  chosen,
  onChoose,
}: {
  address: ServiceAddress;
  query: UseQueryResult<ServiceInstance[]>;
  chosen: string;
  onChoose: (instanceId: string) => void;
}) {
  const { t, i18n } = useTranslation('apm');
  const instances = query.data ?? [];
  const known = instances.filter((instance) => instance.instanceId !== OTHER_DIMENSION).length;
  const grid = TRACKS.join(' ');
  const minWidth = gridMinWidth(TRACKS);

  return (
    <ApmCard
      icon={Boxes}
      tone="text-[var(--blue-ink)]"
      title={t('instances.title')}
      description={t('instances.description')}
      testId="service-instances"
      actions={<span className="font-sans text-xs tabular-nums text-tx-2">{t('service.facts.instances', { count: known })}</span>}
    >
      <ApmQueryState query={query} empty={instances.length === 0} emptyTitle={t('instances.empty')} compact>
        <div role="table" aria-label={t('instances.title')} className="overflow-x-auto">
          <div
            role="row"
            className="sticky top-0 grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0"
            style={{ gridTemplateColumns: grid, minWidth }}
          >
            {COLUMNS.map((column) => (
              <span key={column} role="columnheader" className={cn('truncate', NUMERIC.has(column) && 'text-right')}>
                {t(`instances.columns.${column}`)}
              </span>
            ))}
          </div>
          {instances.map((instance) => {
            const other = instance.instanceId === OTHER_DIMENSION;
            const on = chosen === instance.instanceId;
            const red = instance.red;

            return (
              <OpenRow
                key={instance.instanceId}
                data-apm-instance={instance.instanceId}
                data-chosen={on || undefined}
                onOpen={() => onChoose(on || other ? '' : instance.instanceId)}
                title={other ? t('instances.other_hint') : t(on ? 'instances.all_charts' : 'instances.only_charts')}
                style={{ ...ROW_STYLE, gridTemplateColumns: grid, minWidth }}
                className={cn(
                  'grid cursor-pointer items-center gap-3 border-b border-[var(--row-rule)] px-3 font-sans text-xs outline-none last:border-b-0 hover:bg-bg-2 focus-visible:bg-bg-2',
                  LONG_LIST_ROW_CLASS,
                  on && 'bg-indigo-dim hover:bg-indigo-dim',
                )}
              >
                <span role="cell" className={cn('truncate font-strong', other ? 'text-tx-3' : on ? 'text-indigo-soft' : 'text-tx-0')} title={instance.instanceId}>
                  {other ? t('instances.other') : instance.instanceId}
                </span>
                <span role="cell" className="truncate tabular-nums text-tx-0">
                  {instance.version ?? '—'}
                </span>
                <span
                  role="cell"
                  className="truncate text-tx-0"
                  title={[where(instance), instance.attributes['container.name'], instance.attributes['k8s.node.name'], instance.attributes['cloud.region']].filter(Boolean).join(' · ')}
                >
                  {where(instance) || '—'}
                </span>
                <span role="cell" className="truncate text-tx-0">
                  {runtime(instance) || '—'}
                </span>
                <span role="cell" className="flex items-center justify-end gap-2 tabular-nums text-tx-0">
                  {red ? (
                    <>
                      <Sparkline values={(instance.sparkline ?? []).map((point) => point.requestCount)} variant="bars" zeroBased className="h-4 w-12 text-[var(--chart-1)]" />
                      <span className="w-[72px] text-right">{formatThroughput(throughputOf(red.requestCount, address.range))}</span>
                    </>
                  ) : (
                    '—'
                  )}
                </span>
                <span role="cell" className={cn('text-right tabular-nums', red && red.errorCount > 0 ? 'font-strong text-[var(--red-ink)]' : 'text-tx-0')}>
                  {red && red.requestCount > 0 ? formatRate(red.errorRate) : '—'}
                </span>
                <span role="cell" className="text-right tabular-nums text-tx-0">
                  {formatDuration(red?.p95Time ?? undefined)}
                </span>
                <span role="cell" className="truncate whitespace-nowrap text-right tabular-nums text-tx-0">
                  {formatRelativeMicros(instance.lastSeenAt, i18n.language)}
                </span>
                <span role="cell" className="flex items-center justify-end gap-3">
                  {!other && <InstanceSignals address={address} instance={instance} />}
                </span>
              </OpenRow>
            );
          })}
        </div>
      </ApmQueryState>
    </ApmCard>
  );
}

const LINK = 'font-strong text-tx-2 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none';

/** The instance's logs, by its pod or host, and its metrics, by the label it is told apart by. */
function InstanceSignals({ address, instance }: { address: ServiceAddress; instance: ServiceInstance }) {
  const { t } = useTranslation('apm');
  const handle = {
    namespace: address.namespace,
    service: address.name,
    environment: address.environment,
    from: address.range.from.getTime() * 1_000,
    to: address.range.to.getTime() * 1_000,
  };
  const pod = instance.attributes['k8s.pod.name'];
  const host = instance.attributes['host.name'];
  const own = instanceLabel(instance);
  const selector = `{"service.name"=${JSON.stringify(address.name)},${JSON.stringify(own.label)}=${JSON.stringify(own.value)}}`;

  return (
    <>
      {(pod || host) && (
        <Link to={signalHref('logs', handle, pod ? { pod } : { host: host! })} data-apm-instance-logs="" className={LINK}>
          {t('service.related.logs')}
        </Link>
      )}
      <Link to={signalHref('metrics', handle, { promql: selector })} data-apm-instance-metrics="" className={LINK}>
        {t('service.related.metrics')}
      </Link>
    </>
  );
}

/** The label an instance is told apart by: its pod or host where its id is one, else its SDK's instance id. */
export function instanceLabel(instance: ServiceInstance): RuntimeInstance {
  const { attributes, instanceId: value } = instance;
  if (attributes['k8s.pod.name'] === value) return { label: 'k8s.pod.name', value };
  if (attributes['host.name'] === value) return { label: 'host.name', value };
  return { label: 'service.instance.id', value };
}

/** Where an instance runs: its Kubernetes workload and pod, else its host. */
function where(instance: ServiceInstance): string {
  const { attributes } = instance;
  const workload = attributes['k8s.deployment.name'] ?? attributes['k8s.statefulset.name'];
  const pod = attributes['k8s.pod.name'];

  if (workload || pod) return [attributes['k8s.namespace.name'], workload, pod].filter(Boolean).join(' / ');
  return attributes['host.name'] ?? '';
}

function runtime(instance: ServiceInstance): string {
  const { attributes } = instance;
  return [attributes['process.runtime.name'], attributes['process.runtime.version']].filter(Boolean).join(' ');
}
