import { useQuery } from '@tanstack/react-query';
import { AlertTriangle, ExternalLink, Hourglass, ListTree } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ApmQueryParams } from '@/api/apm';
import { apmMapApi, apmMapQueryKeys, type MapResponse } from '@/api/apmMap';
import { cn } from '@/shell/lib/cn';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { RedCharts } from '../charts/RedCharts';
import { tracesHref } from '../endpointFilters';
import { formatCount, formatDuration, formatRate } from '../format';
import { ApmDrawer } from '../frame/ApmDrawer';
import { ApmCard, ApmQueryState } from '../frame/Page';
import { useAddressedDrawer } from '../frame/useApmScope';

const ROW = 'grid h-8 w-full grid-cols-[minmax(0,1fr)_72px_64px_72px] items-center gap-3 border-b border-bd-0 px-3 text-left font-sans text-xs last:border-b-0';

/** The ends of a call from its id, `<source>-><target>`. */
export function callEnds(id: string): [string, string] | null {
  const at = id.indexOf('->');
  return at > 0 ? [id.slice(0, at), id.slice(at + 2)] : null;
}

/**
 * A call of the map opened: what it carried over time, what was asked
 * along it, how long a topic's messages waited, and the errors of a service
 * called, each opening over the map. The spans of the call open in Traces.
 */
export function CallDrawer({ map, params, orgId }: { map: MapResponse | undefined; params: ApmQueryParams; orgId: string }) {
  const { t } = useTranslation('apm');
  const drawer = useAddressedDrawer('call');
  const error = useAddressedDrawer('error');
  const ends = drawer.value ? callEnds(drawer.value) : null;
  const edge = map?.edges.find((item) => item.id === drawer.value);
  const name = (id: string) => map?.nodes.find((node) => node.id === id)?.name ?? id.slice(id.indexOf(':') + 1);
  const wanted = ends ? { ...params, source: ends[0], target: ends[1] } : null;

  const detail = useQuery({
    queryKey: apmMapQueryKeys.edge(orgId, wanted ?? { source: '', target: '' }),
    queryFn: () => apmMapApi.edge(wanted!),
    enabled: Boolean(orgId && wanted),
  });

  const data = detail.data;
  const kind = data?.kind ?? edge?.kind;
  const [source, target] = ends ?? ['', ''];
  const range = data ? data.meta.range : { from: params.from ?? 0, to: params.to ?? 0 };
  // Whose spans the call is among: the service called, else the caller.
  const spansOf = target.startsWith('service:') ? name(target) : source.startsWith('service:') ? name(source) : null;

  return (
    <ApmDrawer
      open={Boolean(ends)}
      onClose={() => drawer.close()}
      testId="call"
      title={ends ? `${name(source)} → ${name(target)}` : t('map.call.title')}
      subtitle={kind ? t(`map.call.kinds.${kind}`) : undefined}
      actions={
        spansOf ? (
          <Link
            to={tracesHref([{ field: 'service.name', op: '=', value: spansOf }], range, 'spans')}
            className="inline-flex items-center gap-1 font-sans text-xs font-strong text-tx-2 hover:text-tx-0"
          >
            {t('endpoint.in_traces')}
            <ExternalLink aria-hidden className="h-3 w-3" />
          </Link>
        ) : undefined
      }
    >
      <ApmQueryState query={detail} empty={false}>
        {data && (
          <>
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1 font-sans text-xs text-tx-2" data-apm-call-figures="">
              <span>
                {t('map.call.requests')} <span className="tabular-nums text-tx-0">{formatCount(data.red.requestCount)}</span>
              </span>
              <span>
                {t('map.call.error_rate')}{' '}
                <span className={cn('tabular-nums', data.red.errorCount > 0 ? 'text-[var(--red-ink)]' : 'text-tx-0')}>{formatRate(data.red.errorRate)}</span>
              </span>
              <span>
                P95 <span className="tabular-nums text-tx-0">{formatDuration(data.red.p95Time ?? undefined)}</span>
              </span>
            </div>
            <RedCharts trend={data.trend} shiftMicros={0} resolution={data.meta.resolution} versions={[]} range={range} cursorScope="call" />
            {(kind === 'publish' || kind === 'consume') && <MessageWait source={source} target={target} range={range} orgId={orgId} name={name} />}
            <ApmCard
              icon={ListTree}
              tone="text-[var(--blue-ink)]"
              title={t(data.everyCaller ? 'map.call.endpoints' : 'map.call.operations')}
              description={data.everyCaller ? t('map.call.every_caller') : undefined}
              testId="call-operations"
            >
              {data.operations.length === 0 ? (
                <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('map.call.no_operations')}</p>
              ) : (
                data.operations.map((operation) => (
                  <div key={`${operation.kind ?? ''}:${operation.name}`} data-apm-call-operation={operation.name} className={ROW}>
                    {operation.name ? <TraceOperationName operation={operation.name} className="min-w-0 text-tx-0" /> : <span className="text-tx-3">{t('map.call.unnamed')}</span>}
                    <span className="text-right tabular-nums text-tx-1">{formatCount(operation.red.requestCount)}</span>
                    <span className={cn('text-right tabular-nums', operation.red.errorCount > 0 ? 'text-[var(--red-ink)]' : 'text-tx-3')}>{formatRate(operation.red.errorRate)}</span>
                    <span className="text-right tabular-nums text-tx-1">{formatDuration(operation.red.p95Time ?? undefined)}</span>
                  </div>
                ))
              )}
            </ApmCard>
            {data.errors.length > 0 && (
              <ApmCard icon={AlertTriangle} tone="text-[var(--red-ink)]" title={t('map.call.errors', { service: name(target) })} testId="call-errors">
                {data.errors.map((group) => (
                  <button
                    key={group.error.fingerprint}
                    type="button"
                    data-apm-call-error={group.error.fingerprint}
                    onClick={() => error.open(group.error.fingerprint)}
                    className="flex h-8 w-full items-center gap-2 border-b border-bd-0 px-3 text-left font-sans text-xs outline-none last:border-b-0 hover:bg-bg-2 focus-visible:bg-bg-2"
                  >
                    <span className="shrink-0 font-strong text-[var(--red-ink)]">{group.error.errorType}</span>
                    <span className="truncate text-tx-2">{group.representativeMessage ?? ''}</span>
                    <span className="ml-auto shrink-0 tabular-nums text-tx-1">{formatCount(group.occurrenceCount)}</span>
                  </button>
                ))}
              </ApmCard>
            )}
          </>
        )}
      </ApmQueryState>
    </ApmDrawer>
  );
}

/** How long the topic's messages waited between publishing and consuming, from a sample of the trace store. */
function MessageWait({ source, target, range, orgId, name }: { source: string; target: string; range: { from: number; to: number }; orgId: string; name: (id: string) => string }) {
  const { t } = useTranslation('apm');
  const topic = name(source.startsWith('topic:') ? source : target);
  const consumer = source.startsWith('topic:') ? name(target) : undefined;
  const params = { from: range.from, to: range.to, topic, ...(consumer ? { consumer } : {}) };
  const query = useQuery({
    queryKey: apmMapQueryKeys.messagingLatency(orgId, params),
    queryFn: () => apmMapApi.messagingLatency(params),
    enabled: Boolean(orgId && topic),
    staleTime: 60_000,
  });
  const data = query.data;

  return (
    <ApmCard
      icon={Hourglass}
      tone="text-[var(--orange-ink)]"
      title={t('map.call.wait.title')}
      description={data && data.sampled > 0 ? t('map.call.wait.description', { count: data.sampled, matched: data.matched }) : t('map.call.wait.about')}
      testId="call-wait"
    >
      <ApmQueryState query={query} empty={!data || data.matched === 0} emptyTitle={t(data?.unavailable ? 'map.call.wait.unavailable' : 'map.call.wait.none')} compact>
        <div className="grid grid-cols-3 gap-px bg-bd-0 font-sans text-xs" data-apm-call-wait="">
          {(['p50', 'p95', 'max'] as const).map((key) => (
            <div key={key} className="bg-[var(--functional-surface)] px-3 py-2">
              <div className="text-tx-2">{t(`map.call.wait.${key}`)}</div>
              <div className="mt-0.5 text-base font-strong tabular-nums text-tx-0">{formatDuration(data?.[`${key}Time`] ?? undefined)}</div>
            </div>
          ))}
        </div>
      </ApmQueryState>
    </ApmCard>
  );
}
