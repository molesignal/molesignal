import { useQuery } from '@tanstack/react-query';
import { ChevronDown, ChevronRight, Database, LogIn, LogOut, Share2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import type { DependencySummary, ServiceDetailResponse } from '@/api/apm';
import { apmMapApi, apmMapQueryKeys, type MapEdge, type MapResponse } from '@/api/apmMap';
import type { HealthLines } from '@/api/apmSettings';
import { cn } from '@/shell/lib/cn';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { formatCount, formatDuration, formatRate, formatSigned } from '../format';
import { dependencyScore, type ScoreLevel, slowStatements, type TargetFigures, targetFigures } from './dependencyHealth';
import type { ServiceAddress } from './useService';
import { ApmCard, ApmQueryState } from '../frame/Page';
import { useAddressedDrawer } from '../frame/useApmScope';
import { CallDrawer } from '../map/CallDrawer';
import { nodeFigures } from '../map/figures';
import { type CanvasEdge, type CanvasNode, MapCanvas } from '../map/MapCanvas';
import { nodeHealth, nodeShape } from '../map/MapNodes';
import { apmQueryParams, OTHER_DIMENSION, servicePath } from '../scope';
import { useApmSettings } from '../settings/useApmSettings';

const GRID = 'grid-cols-[minmax(0,1fr)_96px_72px_64px_72px_56px]';
const COMPARED_GRID = 'grid-cols-[minmax(0,1fr)_96px_72px_64px_72px_56px_120px]';

/** Each health level in the ink of what it says. */
const SCORE_TEXT: Record<ScoreLevel, string> = {
  good: 'text-[var(--green-ink)]',
  warning: 'text-[var(--orange-ink)]',
  poor: 'text-[var(--red-ink)]',
};

interface TargetGroup {
  key: string;
  category: DependencySummary['dependency']['category'];
  target: string;
  requests: number;
  errors: number;
  totalTime: number;
  operations: DependencySummary[];
  figures: TargetFigures;
}

/** Calls to one target, its operations under it, the target with the most time first. */
function groupByTarget(dependencies: readonly DependencySummary[]): TargetGroup[] {
  const groups = new Map<string, TargetGroup>();

  for (const item of dependencies) {
    const key = `${item.dependency.category}:${item.dependency.target}`;
    const group = groups.get(key) ?? {
      key,
      category: item.dependency.category,
      target: item.dependency.target,
      requests: 0,
      errors: 0,
      totalTime: 0,
      operations: [],
      figures: targetFigures([]),
    };
    group.requests += item.red.requestCount;
    group.errors += item.red.errorCount;
    group.totalTime += item.totalTime;
    group.operations.push(item);
    groups.set(key, group);
  }

  return [...groups.values()]
    .map((group) => ({
      ...group,
      operations: group.operations.sort((a, b) => b.totalTime - a.totalTime),
      figures: targetFigures(group.operations),
    }))
    .sort((a, b) => b.totalTime - a.totalTime);
}

/**
 * What the service depends on and what depends on it, as ARMS's upstream /
 * downstream view: the service among its neighbours, the services that call
 * it, and its calls out grouped by target (a database, a cache, a queue,
 * another service) with each operation under its target, the target's
 * health, and its calls that ran out of time or were sent again. With a
 * stretch compared, each target says how its failures and P95 moved. The
 * slowest database statements close the page.
 */
export function ServiceDependencies({ address, detail, compared }: { address: ServiceAddress; detail: ServiceDetailResponse; compared?: ServiceDetailResponse | undefined }) {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const call = useAddressedDrawer('call');
  // The service among what calls it and what it calls, as the APM map draws them.
  const mapParams = apmQueryParams({ ...address.scope, version: '' }, address.range, address.environment ? { environment: address.environment } : {});
  const map = useQuery({
    queryKey: apmMapQueryKeys.map(address.orgId, mapParams),
    queryFn: () => apmMapApi.map(mapParams),
    enabled: Boolean(address.orgId),
  });
  const seconds = Math.max(1, (address.range.to.getTime() - address.range.from.getTime()) / 1_000);
  const lines = useApmSettings().health;
  const near = React.useMemo(() => neighbours(map.data, `service:${address.name}`, seconds, lines), [address.name, lines, map.data, seconds]);
  const groups = React.useMemo(() => groupByTarget(detail.dependencies), [detail.dependencies]);
  const before = React.useMemo(() => (compared ? new Map(groupByTarget(compared.dependencies).map((group) => [group.key, group.figures])) : null), [compared]);
  const statements = React.useMemo(() => slowStatements(detail.dependencies), [detail.dependencies]);
  const grid = before ? COMPARED_GRID : GRID;
  const [open, setOpen] = React.useState<ReadonlySet<string>>(new Set());

  const toggle = (key: string) =>
    setOpen((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const openService = (name: string) =>
    navigate(
      servicePath(
        {
          namespace: address.namespace,
          name,
          environment: address.environment,
        },
        'dependencies',
        address.searchParams,
      ),
    );

  return (
    <div className="space-y-[12px]">
      <ApmCard icon={Share2} tone="text-[var(--chart-2)]" title={t('dependencies.map')} description={t('dependencies.map_description')} testId="neighbourhood" bodyClassName="h-[300px]">
        <ApmQueryState query={map} empty={near.edges.length === 0} emptyTitle={t('dependencies.map_empty')} compact>
          <MapCanvas
            nodes={near.nodes}
            edges={near.edges}
            onNode={(id) => {
              const name = id.startsWith('service:') ? id.slice('service:'.length) : null;
              if (name && name !== address.name) openService(name);
            }}
            onEdge={(edge) => call.open(edge.id)}
            onPane={() => undefined}
          />
        </ApmQueryState>
      </ApmCard>
      <div className="grid min-w-0 gap-[12px] xl:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
        <ApmCard icon={LogIn} tone="text-[var(--chart-7)]" title={t('dependencies.upstream')} description={t('dependencies.upstream_description')} testId="upstream">
          {near.callers.length === 0 ? (
            <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('dependencies.no_upstream')}</p>
          ) : (
            near.callers.map((edge) => (
              <button
                key={edge.id}
                type="button"
                data-apm-caller={edge.source}
                onClick={() => call.open(edge.id)}
                className="grid h-8 w-full grid-cols-[minmax(0,1fr)_64px_64px] items-center gap-3 border-b border-bd-0 px-3 text-left font-sans text-xs outline-none last:border-b-0 hover:bg-bg-2 focus-visible:bg-bg-2"
              >
                <span className={cn('truncate', edge.change === 'gone' ? 'text-tx-3 line-through' : 'text-tx-0')}>{nameOf(map.data, edge.source)}</span>
                <span className={cn('text-right tabular-nums', edge.red.errorCount > 0 ? 'text-[var(--red-ink)]' : 'text-tx-3')}>{formatRate(edge.red.errorRate)}</span>
                <span className="text-right tabular-nums text-tx-1">{formatDuration(edge.red.p95Time ?? undefined)}</span>
              </button>
            ))
          )}
        </ApmCard>
        <ApmCard icon={LogOut} tone="text-[var(--chart-2)]" title={t('dependencies.downstream')} description={t('dependencies.downstream_description')} testId="downstream-table">
          {groups.length === 0 ? (
            <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('dependencies.no_downstream')}</p>
          ) : (
            <div role="table" aria-label={t('dependencies.downstream')}>
              <div role="row" className={cn('grid h-8 items-center gap-3 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0', grid)}>
                <span role="columnheader">{t('dependencies.columns.target')}</span>
                <span role="columnheader">{t('dependencies.columns.category')}</span>
                <span role="columnheader" className="text-right">
                  {t('dependencies.columns.requests')}
                </span>
                <span role="columnheader" className="text-right">
                  {t('dependencies.columns.error_rate')}
                </span>
                <span role="columnheader" className="text-right">
                  {t('dependencies.columns.p95')}
                </span>
                <span role="columnheader" className="text-right">
                  {t('dependencies.columns.health')}
                </span>
                {before && <span role="columnheader">{t('dependencies.columns.change')}</span>}
              </div>
              {groups.map((group) => {
                const expanded = open.has(group.key);
                const other = group.target === OTHER_DIMENSION;
                const earlier = before?.get(group.key) ?? null;
                const health = dependencyScore(group.figures, earlier);

                return (
                  <React.Fragment key={group.key}>
                    <button
                      type="button"
                      role="row"
                      aria-expanded={expanded}
                      data-apm-dependency={group.target}
                      onClick={() => toggle(group.key)}
                      className={cn('grid h-8 w-full items-center gap-3 border-b border-bd-0 px-3 text-left font-sans text-xs outline-none hover:bg-bg-2 focus-visible:bg-bg-2', grid)}
                    >
                      <span className="flex min-w-0 items-center gap-1.5">
                        {expanded ? (
                          <ChevronDown aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
                        ) : (
                          <ChevronRight aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
                        )}
                        <span className={cn('truncate font-strong', other ? 'text-tx-3' : 'text-tx-0')} title={other ? t('overflow.other_hint') : group.target}>
                          {other ? t('overflow.other') : group.target}
                        </span>
                        <span className="shrink-0 text-tx-3">
                          {t('dependencies.operations', {
                            count: group.operations.length,
                          })}
                        </span>
                        {group.figures.timeouts > 0 && <span className="shrink-0 font-strong text-[var(--orange-ink)]">{t('dependencies.timeouts', { count: group.figures.timeouts })}</span>}
                        {group.figures.retries > 0 && <span className="shrink-0 text-tx-2">{t('dependencies.retries', { count: group.figures.retries })}</span>}
                      </span>
                      <span className="truncate text-tx-2">{t(`dependencies.category.${group.category}`)}</span>
                      <span className="text-right tabular-nums text-tx-1">{formatCount(group.requests)}</span>
                      <span className={cn('text-right tabular-nums', group.errors > 0 ? 'text-[var(--red-ink)]' : 'text-tx-3')}>
                        {formatRate(group.requests > 0 ? group.errors / group.requests : 0)}
                      </span>
                      <span className="text-right tabular-nums text-tx-1" title={t('dependencies.slowest_operation')}>
                        {formatDuration(group.figures.p95 ?? undefined)}
                      </span>
                      <span
                        data-apm-dependency-health={health.level}
                        className={cn('text-right font-strong tabular-nums', SCORE_TEXT[health.level])}
                        title={health.reasons.length > 0 ? health.reasons.map((reason) => t(`dependencies.score.${reason.key}`, { points: reason.points })).join('\n') : t('dependencies.score.none')}
                      >
                        {health.score}
                      </span>
                      {before && <Change now={group.figures} before={earlier} />}
                    </button>
                    {expanded &&
                      group.operations.map((item) => (
                        <div
                          key={`${group.key}:${item.dependency.operation ?? ''}:${item.version ?? ''}`}
                          role="row"
                          className={cn('grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--page-canvas)] pl-9 pr-3 font-sans text-xs', grid)}
                        >
                          <span className="flex min-w-0 items-center gap-1.5">
                            {item.dependency.operation ? (
                              <TraceOperationName mono={false} operation={item.dependency.operation} className="min-w-0 text-tx-1" />
                            ) : (
                              <span className="truncate text-tx-3">{t('dependencies.no_operation')}</span>
                            )}
                            {(item.red.timeoutCount ?? 0) > 0 && (
                              <span className="shrink-0 font-strong text-[var(--orange-ink)]">{t('dependencies.timeouts', { count: item.red.timeoutCount ?? 0 })}</span>
                            )}
                            {(item.red.retryCount ?? 0) > 0 && <span className="shrink-0 text-tx-2">{t('dependencies.retries', { count: item.red.retryCount ?? 0 })}</span>}
                          </span>
                          <span />
                          <span className="text-right tabular-nums text-tx-1">{formatCount(item.red.requestCount)}</span>
                          <span className={cn('text-right tabular-nums', item.red.errorCount > 0 ? 'text-[var(--red-ink)]' : 'text-tx-3')}>
                            {formatRate(item.red.errorRate)}
                          </span>
                          <span className="text-right tabular-nums text-tx-1">{formatDuration(item.red.p95Time)}</span>
                          <span />
                          {before && <span />}
                        </div>
                      ))}
                  </React.Fragment>
                );
              })}
            </div>
          )}
        </ApmCard>
      </div>
      <CallDrawer map={map.data} params={mapParams} orgId={address.orgId} />
      {statements.length > 0 && (
        <ApmCard icon={Database} tone="text-[var(--blue-ink)]" title={t('dependencies.slow_sql')} description={t('dependencies.slow_sql_description')} testId="slow-sql">
          {statements.map((item) => (
            <div
              key={`${item.dependency.target}:${item.dependency.operation}:${item.version ?? ''}`}
              data-apm-slow-sql={item.dependency.operation}
              className="grid h-8 grid-cols-[minmax(0,1fr)_120px_72px_64px_72px] items-center gap-3 border-b border-bd-0 px-3 font-sans text-xs last:border-b-0"
            >
              <span className="truncate text-tx-0" title={item.dependency.operation}>
                {item.dependency.operation}
              </span>
              <span className="truncate text-tx-2" title={item.dependency.target}>
                {item.dependency.target}
              </span>
              <span className="text-right tabular-nums text-tx-1">{formatCount(item.red.requestCount)}</span>
              <span className={cn('text-right tabular-nums', item.red.errorCount > 0 ? 'text-[var(--red-ink)]' : 'text-tx-3')}>{formatRate(item.red.errorRate)}</span>
              <span className="text-right font-strong tabular-nums text-tx-0">{formatDuration(item.red.p95Time)}</span>
            </div>
          ))}
        </ApmCard>
      )}
    </div>
  );
}

/**
 * The node `id` among what calls it and what it calls, as the canvas draws
 * them; and the calls into it, the busiest first.
 */
function neighbours(map: MapResponse | undefined, id: string, seconds: number, lines: HealthLines): { nodes: CanvasNode[]; edges: CanvasEdge[]; callers: MapEdge[] } {
  const touching = (map?.edges ?? []).filter((edge) => edge.source === id || edge.target === id);
  const ends = new Set([id, ...touching.flatMap((edge) => [edge.source, edge.target])]);
  const nodes = (map?.nodes ?? [])
    .filter((node) => ends.has(node.id))
    .map<CanvasNode>((node) => ({
      id: node.id,
      data: {
        name: node.name,
        shape: nodeShape(node),
        health: nodeHealth(node, lines),
        figures: nodeFigures(node.red, seconds),
        change: node.change,
        dimmed: false,
        chosen: node.id === id,
      },
    }));

  return {
    nodes,
    edges: touching.map((edge) => ({ ...edge, merged: [edge.id], dimmed: false, critical: false })),
    callers: touching.filter((edge) => edge.target === id).sort((left, right) => right.red.requestCount - left.red.requestCount),
  };
}

function nameOf(map: MapResponse | undefined, id: string): string {
  return map?.nodes.find((node) => node.id === id)?.name ?? id.slice(id.indexOf(':') + 1);
}

/** How a target's failures and P95 moved against the stretch compared with, each in the ink of better or worse. */
function Change({ now, before }: { now: TargetFigures; before: TargetFigures | null }) {
  const { t } = useTranslation('apm');

  if (!before || before.requests === 0) return <span className="truncate text-tx-3">{t('dependencies.new_target')}</span>;

  const rate = (now.requests > 0 ? now.errors / now.requests : 0) - before.errors / before.requests;
  const p95 = now.p95 !== null && before.p95 !== null ? now.p95 - before.p95 : null;
  const ink = (value: number) => (value > 0 ? 'text-[var(--orange-ink)]' : value < 0 ? 'text-[var(--green-ink)]' : 'text-tx-3');
  // A figure that did not move says nothing.
  const said = (value: number | null, text: string) => (value ? text : '—');

  return (
    <span className="flex min-w-0 items-center gap-2 tabular-nums" data-apm-dependency-change="">
      <span className={ink(rate)}>{said(rate, formatSigned(rate, true))}</span>
      <span className={ink(p95 ?? 0)}>{said(p95, formatSigned(p95 ?? undefined, false, true))}</span>
    </span>
  );
}
