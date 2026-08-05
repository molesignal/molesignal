import { useQuery } from '@tanstack/react-query';
import { ArrowLeft, ArrowRight, Eye, EyeOff, Maximize2, Minimize2, RotateCcw, Settings } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as dashboardsApi from '@/api/dashboards';
import * as incidentsApi from '@/api/incidents';
import * as webApi from '@/api/web';
import { Dot, Pill, uiLabelClass, uiLabelStrongClass, uiTableHeaderClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { LogoMark } from '@/shell/LogoMark';
import { NocKpi, NocPanel } from '@/shell/NocPanel';
import { QueryState, queryStateFor } from '@/shell/query/State';
import { ServiceMap, type ServiceEdge, type ServiceNode } from '@/shell/svgCharts';
import { getFullscreenDashboard } from '@/shell/wallboard';
import { useAuthStore } from '@/stores/auth';
import { useNocLayoutStore, type NocPanelId } from '@/stores/useNocLayoutStore';
import type { Incident } from '@/types/alerting';
import type { Dashboard } from '@/types/dashboard';
import { SeverityRail } from '@/viz/SeverityRail';

// Phase 4 status color logic: the hex constants below migrate from the
// legacy Terminal hex set to values that match the new default palette;
// ideally these would read from `var(...)` but the SVG renderer here takes
// literal strings. (Severity → bar color lives in @/viz/SeverityRail.)
// Phase 4+ brand tokens — reads from the active palette so NOC
// stays correct in dark/light and any future palette swap.
const STATE_COLOR = { healthy: 'var(--green)', degraded: 'var(--yellow)', error: 'var(--red)' } as const;

function lastHourRange(): { from: string; to: string } {
  const now = new Date();
  const from = new Date(now.getTime() - 60 * 60 * 1000);
  return { from: from.toISOString(), to: now.toISOString() };
}

function statusOf(errRate: number): 'healthy' | 'degraded' | 'error' {
  if (errRate >= 0.05) return 'error';
  if (errRate >= 0.01) return 'degraded';
  return 'healthy';
}

function ageOf(createdAtMicros: number | undefined): string {
  if (!createdAtMicros) return '—';
  const diff = Date.now() - Math.floor(createdAtMicros / 1000);
  if (diff < 60_000) return `${Math.round(diff / 1000)}s`;
  if (diff < 3_600_000) return `${Math.round(diff / 60_000)}m`;
  return `${Math.round(diff / 3_600_000)}h`;
}

export function Noc() {
  const { t } = useTranslation('shell');
  const nav = useNavigate();
  const orgId = useAuthStore((s) => s.ctx?.org_id ?? '');
  const [now, setNow] = React.useState(new Date());
  const [fullscreenDashboard, setFullscreenDashboardState] = React.useState(() => getFullscreenDashboard(orgId));
  const [editing, setEditing] = React.useState(false);
  const nocPanels = useNocLayoutStore((s) => s.panels);
  // Per-panel grid props derived from the persisted layout: column span +
  // visibility via class, position via CSS `order` (no DnD dependency needed).
  const slotProps = (id: NocPanelId) => {
    const index = nocPanels.findIndex((p) => p.id === id);
    const cfg = index >= 0 ? nocPanels[index] : undefined;
    return {
      className: cn('h-full', cfg?.span === 4 ? 'col-span-4' : 'col-span-2', !cfg?.visible && 'hidden'),
      style: { order: index + 1 } as React.CSSProperties,
    };
  };

  React.useEffect(() => {
    setFullscreenDashboardState(getFullscreenDashboard(orgId));
  }, [orgId]);

  React.useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 1000);
    return () => window.clearInterval(id);
  }, []);

  const returnToConsole = React.useCallback(() => {
    nav(fullscreenDashboard?.dashboardId ? `/dashboards/${fullscreenDashboard.dashboardId}` : '/home');
  }, [fullscreenDashboard?.dashboardId, nav]);

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        returnToConsole();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [returnToConsole]);

  const range = React.useMemo(lastHourRange, []);
  const topoQuery = useQuery({
    queryKey: ['web', 'topology', range.from, range.to],
    queryFn: () => webApi.topology(range.from, range.to),
    refetchInterval: 30_000,
  });
  const incidentsQuery = useQuery({
    queryKey: ['alerts', 'incidents'],
    queryFn: () => incidentsApi.list(),
    refetchInterval: 15_000,
  });
  const dashboardQuery = useQuery({
    queryKey: ['dashboards', 'fullscreen', fullscreenDashboard?.dashboardId],
    queryFn: () => dashboardsApi.get(fullscreenDashboard!.dashboardId),
    enabled: !!fullscreenDashboard?.dashboardId,
    refetchInterval: 30_000,
  });

  const topology = topoQuery.data;
  const incidents = incidentsQuery.data ?? [];

  // Aggregate KPIs from topology edges (real data).
  const allEdges = topology?.edges ?? [];
  const edgeRps = allEdges.reduce((a, e) => a + e.rps, 0);
  const serviceRps = (topology?.nodes ?? []).reduce((a, node) => a + node.rps, 0);
  const totalRps = edgeRps > 0 ? edgeRps : serviceRps;
  const totalErrors =
    edgeRps > 0
      ? allEdges.reduce((a, e) => a + e.rps * e.err_rate, 0)
      : (topology?.nodes ?? []).reduce((a, node) => a + node.rps * node.error_rate, 0);
  const errRate = totalRps > 0 ? totalErrors / totalRps : 0;
  const maxP95 = Math.max(
    allEdges.reduce((a, e) => Math.max(a, e.p95_ms), 0),
    (topology?.nodes ?? []).reduce((a, node) => Math.max(a, node.p95_ms), 0),
  );
  const trafficServices = React.useMemo(
    () =>
      [...(topology?.nodes ?? [])]
        .filter((service) => service.rps > 0)
        .sort((a, b) => b.rps - a.rps)
        .slice(0, 8),
    [topology?.nodes],
  );

  const nodes: ServiceNode[] = React.useMemo(() => {
    if (!topology) return [];
    return topology.nodes.map((n, i) => ({
      id: n.id,
      short: n.name.slice(0, 4).toUpperCase(),
      name: n.name,
      qps: Math.round(n.rps),
      x: 110 + (i % 4) * 190,
      y: 80 + Math.floor(i / 4) * 90,
      status: statusOf(n.error_rate),
    }));
  }, [topology]);

  const edges: ServiceEdge[] = React.useMemo(() => {
    if (!topology) return [];
    return topology.edges.map((e) => ({ from: e.source, to: e.target }));
  }, [topology]);

  // Per-KPI sparklines and traffic series would each require their own /query
  // call; until the NOC wallboard wires those in, leave the chart slots empty
  // rather than fabricate series.
  const emptyData: number[] = [];

  const time = now.toLocaleTimeString('en-US', { hour12: false });
  const date = now.toLocaleDateString('en-CA');

  const activeIncidents = incidents.filter((i) => i.status === 'open' || i.status === 'acknowledged');
  const criticalCount = activeIncidents.filter((i) => i.severity === 'critical').length;
  const warningCount = activeIncidents.filter((i) => i.severity === 'warning').length;
  const topologyDataState: 'loading' | 'error' | 'empty' | 'ready' = topoQuery.isLoading
    ? 'loading'
    : topoQuery.isError
      ? 'error'
      : totalRps > 0
        ? 'ready'
        : 'empty';
  const topologyFallback =
    topologyDataState === 'loading'
      ? t('pages.noc.kpis.connecting')
      : topologyDataState === 'error'
        ? t('pages.noc.kpis.unavailable')
        : t('pages.noc.kpis.no_traffic');
  const incidentStatusTone =
    incidentsQuery.isError || criticalCount > 0
      ? 'red'
      : incidentsQuery.isLoading || activeIncidents.length > 0
        ? 'yellow'
        : 'green';
  const incidentStatusLabel = incidentsQuery.isLoading
    ? t('pages.noc.status_bar.incidents_connecting')
    : incidentsQuery.isError
      ? t('pages.noc.status_bar.incidents_unavailable')
      : activeIncidents.length > 0
        ? t('pages.noc.status_bar.incidents_active')
        : t('pages.noc.status_bar.no_active_incidents');
  const topologyStatusTone =
    topologyDataState === 'error' ? 'red' : topologyDataState === 'loading' ? 'yellow' : 'green';
  const topologyStatusLabel =
    topologyDataState === 'loading'
      ? t('pages.noc.status_bar.data_connecting')
      : topologyDataState === 'error'
        ? t('pages.noc.status_bar.data_unavailable')
        : topologyDataState === 'empty'
          ? t('pages.noc.status_bar.no_traffic')
          : t('pages.noc.status_bar.data_ok');

  const topoState = queryStateFor({
    isLoading: topoQuery.isLoading,
    isError: topoQuery.isError,
    data: nodes,
  });
  const incidentsState = queryStateFor({
    isLoading: incidentsQuery.isLoading,
    isError: incidentsQuery.isError,
    data: activeIncidents,
  });

  if (fullscreenDashboard?.dashboardId && dashboardQuery.data) {
    return <DashboardWallboard dashboard={dashboardQuery.data} now={now} onReturn={returnToConsole} />;
  }

  return (
    <div className="fixed inset-0 z-[100] flex flex-col bg-bg-0 p-6">
      <div className="flex items-center gap-3 border-b border-bd-0 pb-3">
        <LogoMark size={32} />
        <div>
          <div className={uiLabelClass}>{t('pages.noc.title')}</div>
          <div className="font-sans text-base font-strong text-tx-0">{t('pages.noc.status_line')}</div>
        </div>
        <div className="ml-auto flex items-center gap-6">
          <button
            type="button"
            onClick={() => setEditing((v) => !v)}
            aria-label={t('pages.noc.edit.configure')}
            title={t('pages.noc.edit.configure')}
            className={cn(
              'grid h-8 w-8 place-items-center rounded-md border text-tx-2 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
              editing ? 'border-indigo bg-indigo-dim text-indigo-soft' : 'border-bd-1 bg-bg-2 hover:bg-bg-3',
            )}
          >
            <Settings className="h-4 w-4" />
          </button>
          <div>
            <div className={`text-right ${uiLabelClass}`}>{date}</div>
            <div className="flex items-baseline">
              <span className="font-sans text-[56px] font-display-strong leading-none tracking-tight text-tx-0">{time}</span>
              <span className="ml-2 font-sans text-xs text-tx-3">{t('pages.noc.utc_label')}</span>
            </div>
          </div>
        </div>
      </div>

      {editing && <NocEditBar onDone={() => setEditing(false)} />}

      <div className="my-3 grid flex-1 auto-rows-[minmax(0,1fr)] grid-cols-4 grid-rows-[auto] gap-3 overflow-hidden">
        <NocKpi
          label={t('pages.noc.kpis.req_per_sec')}
          value={topologyDataState === 'ready' ? `${(totalRps / 1000).toFixed(1)}k` : topologyFallback}
          data={emptyData}
          color="var(--chart-7)"
          state={topologyDataState}
        />
        <NocKpi
          label={t('pages.noc.kpis.p95')}
          value={topologyDataState === 'ready' ? `${Math.round(maxP95)}ms` : topologyFallback}
          data={emptyData}
          color="var(--green)"
          state={topologyDataState}
        />
        <NocKpi
          label={t('pages.noc.kpis.err_rate')}
          value={topologyDataState === 'ready' ? `${(errRate * 100).toFixed(2)}%` : topologyFallback}
          data={emptyData}
          color="var(--red)"
          state={topologyDataState}
        />
        <NocKpi
          label={t('pages.noc.kpis.edges')}
          value={topologyDataState === 'ready' ? `${allEdges.length}` : topologyFallback}
          data={emptyData}
          color="var(--chart-7)"
          state={topologyDataState}
        />

        <NocPanel title={t('pages.noc.panels.traffic_title')} {...slotProps('traffic')}>
          {topologyDataState === 'ready' ? (
            <TrafficBreakdown services={trafficServices} />
          ) : (
            <QueryState
              state={topologyDataState}
              loadingLabel={t('pages.noc.panels.data_connecting')}
              emptyLabel={t('pages.noc.panels.traffic_empty')}
              errorLabel={t('pages.noc.panels.data_unavailable')}
            />
          )}
        </NocPanel>

        <NocPanel title={t('pages.noc.panels.incidents_title', { count: activeIncidents.length })} {...slotProps('incidents')}>
          {incidentsState ? (
            <QueryState
              state={incidentsState}
              error={incidentsQuery.error}
              loadingLabel={t('pages.noc.panels.incidents_connecting')}
              emptyLabel={t('pages.noc.panels.incidents_empty')}
              errorLabel={t('pages.noc.panels.incidents_unavailable')}
            />
          ) : (
            <div className="flex flex-col">
              {activeIncidents.slice(0, 6).map((inc: Incident) => (
                <div
                  key={inc.id}
                  className="grid grid-cols-[3px_1fr_auto_auto] items-center gap-3 border-b border-bd-0 py-2.5 last:border-b-0"
                >
                  <SeverityRail severity={inc.severity} className="h-9" />
                  <div>
                    <div className="type-data font-sans text-tx-0">{inc.summary || inc.rule_id}</div>
                    <div className="font-sans text-xs font-semibold tracking-normal text-tx-2">
                      {inc.severity} · {inc.status}
                    </div>
                  </div>
                  <div className="font-sans text-base text-tx-1">{ageOf(inc.created_at)}</div>
                  <Pill tone={inc.severity === 'critical' ? 'red' : inc.severity === 'warning' ? 'yellow' : 'dim'}>
                    {inc.status}
                  </Pill>
                </div>
              ))}
            </div>
          )}
        </NocPanel>

        <NocPanel title={t('pages.noc.panels.topology_title')} {...slotProps('topology')} bodyClassName="p-0">
          {topoState ? (
            <QueryState
              state={topoState}
              error={topoQuery.error}
              loadingLabel={t('pages.noc.panels.data_connecting')}
              emptyLabel={t('pages.noc.panels.topology_empty')}
              errorLabel={t('pages.noc.panels.data_unavailable')}
            />
          ) : (
            <ServiceMap nodes={nodes} edges={edges} height={300} />
          )}
        </NocPanel>

        <NocPanel title={t('pages.noc.panels.health_title')} {...slotProps('health')}>
          {topoState ? (
            <QueryState
              state={topoState}
              error={topoQuery.error}
              loadingLabel={t('pages.noc.panels.data_connecting')}
              emptyLabel={t('pages.noc.panels.health_empty')}
              errorLabel={t('pages.noc.panels.data_unavailable')}
            />
          ) : (
            <div className="font-sans text-xs">
              <div className={`grid grid-cols-[18px_1.6fr_70px_60px_60px] border-b border-bd-1 pb-1.5 ${uiTableHeaderClass}`}>
                <div />
                <div>{t('pages.noc.table.service')}</div>
                <div className="text-right">{t('pages.noc.table.qps')}</div>
                <div className="text-right">{t('pages.noc.table.p95')}</div>
                <div className="text-right">{t('pages.noc.table.err_pct')}</div>
              </div>
              {(topology?.nodes ?? []).slice(0, 12).map((s) => {
                const st = statusOf(s.error_rate);
                return (
                  <div
                    key={s.id}
                    className="grid grid-cols-[18px_1.6fr_70px_60px_60px] items-center border-b border-bd-0 py-1.5 last:border-b-0"
                  >
                    <span
                      className="h-2.5 w-2.5 rounded-full"
                      style={{ background: STATE_COLOR[st], boxShadow: `0 0 8px ${STATE_COLOR[st]}` }}
                    />
                    <div className="text-tx-0">{s.name}</div>
                    <div className="text-right text-tx-1">{Math.round(s.rps).toLocaleString()}</div>
                    <div className="text-right text-tx-1">{Math.round(s.p95_ms)}ms</div>
                    <div className={`text-right ${s.error_rate > 0.01 ? 'text-red-soft' : s.error_rate > 0.001 ? 'text-yellow-soft' : 'text-tx-1'}`}>
                      {(s.error_rate * 100).toFixed(2)}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </NocPanel>
      </div>

      <div className="flex items-center gap-3 overflow-hidden border-t border-bd-0 pt-2 font-sans text-xs font-semibold tracking-normal text-tx-2">
        <span className="flex items-center gap-1">
          <Dot tone={incidentStatusTone} />
          {incidentStatusLabel}
        </span>
        <span className="text-tx-3">·</span>
        <span className="flex items-center gap-1">
          <Dot tone={topologyStatusTone} />
          {topologyStatusLabel}
        </span>
        {topologyDataState === 'ready' && (
          <>
            <span className="text-tx-3">·</span>
            <span>{t('pages.noc.status_bar.services', { count: topology?.nodes.length ?? 0 })}</span>
            <span className="text-tx-3">·</span>
            <span>
              {t('pages.noc.status_bar.req_per_sec_format', { value: `${(totalRps / 1000).toFixed(1)}K` })}
            </span>
            <span className="text-tx-3">·</span>
            <span>{t('pages.noc.status_bar.p95_format', { value: `${Math.round(maxP95)}ms` })}</span>
          </>
        )}
        {(criticalCount > 0 || warningCount > 0) && (
          <>
            <span className="text-tx-3">·</span>
            <span>
              <span className="text-red-soft">{t('pages.noc.status_bar.crit', { count: criticalCount })}</span>
              <span className="text-tx-3"> · </span>
              <span className="text-yellow-soft">{t('pages.noc.status_bar.warn', { count: warningCount })}</span>
            </span>
          </>
        )}
        <span className="ml-auto flex items-center gap-3">
          <button
            type="button"
            onClick={returnToConsole}
            className="inline-flex h-8 items-center gap-1.5 rounded-md border border-bd-1 bg-transparent px-2.5 text-tx-1 transition-colors hover:bg-bg-2 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
          >
            <ArrowLeft className="h-3.5 w-3.5" />
            {t('pages.noc.return_console')}
          </button>
          <span className="text-tx-3">{t('pages.noc.esc_to_exit')}</span>
          <span className="flex items-center gap-1"><Dot tone="green" /> {t('pages.noc.status_bar.live')}</span>
        </span>
      </div>
    </div>
  );
}

function TrafficBreakdown({
  services,
}: {
  services: Array<{ id: string; name: string; rps: number }>;
}) {
  const { t } = useTranslation('shell');
  const maxRps = Math.max(...services.map((service) => service.rps), 1);

  return (
    <div className="flex h-full min-h-[240px] flex-col justify-center gap-3 font-sans text-xs">
      {services.map((service) => (
        <div key={service.id} className="grid grid-cols-[minmax(100px,1fr)_3fr_76px] items-center gap-3">
          <span className="truncate font-strong text-tx-1">{service.name}</span>
          <span className="h-2 overflow-hidden rounded-full bg-bg-3">
            <span
              className="block h-full rounded-full bg-indigo"
              style={{ width: `${Math.max(3, (service.rps / maxRps) * 100)}%` }}
            />
          </span>
          <span className="text-right text-tx-2">
            {t('pages.noc.panels.traffic_rate', { value: service.rps.toFixed(1) })}
          </span>
        </div>
      ))}
    </div>
  );
}

function NocEditBar({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation('shell');
  const panels = useNocLayoutStore((s) => s.panels);
  const move = useNocLayoutStore((s) => s.move);
  const toggleVisible = useNocLayoutStore((s) => s.toggleVisible);
  const cycleSpan = useNocLayoutStore((s) => s.cycleSpan);
  const applyPreset = useNocLayoutStore((s) => s.applyPreset);
  const reset = useNocLayoutStore((s) => s.reset);

  return (
    <div className="mb-1 flex flex-wrap items-center gap-2 rounded-lg border border-bd-1 bg-bg-1 px-3 py-2 font-sans text-xs">
      <span className="font-strong uppercase tracking-normal text-tx-3">{t('pages.noc.edit.presets')}</span>
      {(['platform', 'sre', 'executive'] as const).map((preset) => (
        <button
          key={preset}
          type="button"
          onClick={() => applyPreset(preset)}
          className="rounded border border-bd-1 bg-bg-2 px-2 py-1 font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
        >
          {t(`pages.noc.edit.preset_${preset}`)}
        </button>
      ))}
      <span className="mx-1 h-4 w-px bg-bd-1" />
      {panels.map((p, i) => (
        <div key={p.id} className="flex items-center gap-1 rounded border border-bd-0 bg-bg-2 px-1.5 py-1">
          <span className={cn('mr-1 font-strong', p.visible ? 'text-tx-1' : 'text-tx-3 line-through')}>
            {t(`pages.noc.panel_names.${p.id}`)}
          </span>
          <NocEditBtn onClick={() => move(p.id, -1)} disabled={i === 0} label={t('pages.noc.edit.move_left')}>
            <ArrowLeft className="h-3 w-3" />
          </NocEditBtn>
          <NocEditBtn onClick={() => move(p.id, 1)} disabled={i === panels.length - 1} label={t('pages.noc.edit.move_right')}>
            <ArrowRight className="h-3 w-3" />
          </NocEditBtn>
          <NocEditBtn onClick={() => cycleSpan(p.id)} label={t('pages.noc.edit.wide')}>
            {p.span === 4 ? <Minimize2 className="h-3 w-3" /> : <Maximize2 className="h-3 w-3" />}
          </NocEditBtn>
          <NocEditBtn onClick={() => toggleVisible(p.id)} label={t('pages.noc.edit.show_hide')}>
            {p.visible ? <Eye className="h-3 w-3" /> : <EyeOff className="h-3 w-3" />}
          </NocEditBtn>
        </div>
      ))}
      <div className="ml-auto flex items-center gap-2">
        <button
          type="button"
          onClick={reset}
          className="inline-flex items-center gap-1 rounded px-2 py-1 font-strong text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
        >
          <RotateCcw className="h-3 w-3" /> {t('pages.noc.edit.reset')}
        </button>
        <button
          type="button"
          onClick={onDone}
          className="rounded bg-indigo px-2.5 py-1 font-bold text-white hover:bg-indigo-soft focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo focus-visible:ring-offset-2 focus-visible:ring-offset-bg-0"
        >
          {t('pages.noc.edit.done')}
        </button>
      </div>
    </div>
  );
}

function NocEditBtn({
  children,
  onClick,
  disabled,
  label,
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={label}
      title={label}
      className="grid h-5 w-5 place-items-center rounded text-tx-2 hover:bg-bg-3 hover:text-tx-0 disabled:opacity-30 disabled:hover:bg-transparent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-indigo"
    >
      {children}
    </button>
  );
}

function DashboardWallboard({
  dashboard,
  now,
  onReturn,
}: {
  dashboard: Dashboard;
  now: Date;
  onReturn: () => void;
}) {
  const { t } = useTranslation('shell');
  const panels = dashboardPanels(dashboard);
  const time = now.toLocaleTimeString('en-US', { hour12: false });
  const date = now.toLocaleDateString('en-CA');

  return (
    <div className="fixed inset-0 z-[100] flex flex-col bg-bg-0 p-6">
      <div className="flex items-center gap-3 border-b border-bd-0 pb-3">
        <LogoMark size={32} />
        <div className="min-w-0">
          <div className={uiLabelClass}>{t('pages.noc.dashboard_wallboard.title')}</div>
          <div className="truncate font-sans text-base font-strong text-tx-0">
            {dashboard.title} {t('pages.noc.dashboard_wallboard.subtitle_suffix')}
          </div>
        </div>
        <div className="ml-auto flex items-center gap-8">
          <div>
            <div className={'text-right ' + uiLabelClass}>{date}</div>
            <div className="flex items-baseline">
              <span className="font-sans text-[56px] font-display-strong leading-none tracking-tight text-tx-0">{time}</span>
              <span className="ml-2 font-sans text-xs text-tx-3">{t('pages.noc.utc_label')}</span>
            </div>
          </div>
        </div>
      </div>

      <div className="my-3 grid flex-1 grid-cols-4 auto-rows-fr gap-3 overflow-hidden">
        {panels.length > 0 ? panels.slice(0, 12).map((panel, index) => (
          <DashboardWallboardPanel key={panel.id} panel={panel} index={index} />
        )) : (
          <div className="col-span-4 grid place-items-center rounded-lg border border-dashed border-bd-1 bg-bg-1 font-sans text-sm text-tx-2">
            {t('pages.noc.dashboard_wallboard.no_panels')}
          </div>
        )}
      </div>

      <div className="flex items-center gap-3 overflow-hidden border-t border-bd-0 pt-2 font-sans text-xs font-semibold tracking-normal text-tx-2">
        <span>{t('pages.noc.dashboard_wallboard.panels_count', { count: panels.length })}</span>
        <span className="text-tx-3">·</span>
        <span>{t('pages.noc.dashboard_wallboard.version', { version: dashboard.version })}</span>
        <span className="text-tx-3">·</span>
        <span>{dashboard.uid}</span>
        <span className="ml-auto flex items-center gap-3">
          <button
            type="button"
            onClick={onReturn}
            className="inline-flex h-8 items-center gap-1.5 rounded-md border border-bd-1 bg-transparent px-2.5 text-tx-1 transition-colors hover:bg-bg-2 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
          >
            <ArrowLeft className="h-3.5 w-3.5" />
            {t('pages.noc.return_console')}
          </button>
          <span className="text-tx-3">{t('pages.noc.esc_to_exit')}</span>
          <span className="flex items-center gap-1"><Dot tone="green" /> {t('pages.noc.status_bar.live')}</span>
        </span>
      </div>
    </div>
  );
}

interface WallboardPanel {
  id: string;
  title: string;
  type: string;
  query: string;
  span: number;
}

function dashboardPanels(dashboard: Dashboard): WallboardPanel[] {
  const rawPanels = Array.isArray((dashboard.model as { panels?: unknown[] }).panels)
    ? (dashboard.model as { panels: unknown[] }).panels
    : [];
  return rawPanels.map((raw, index) => {
    const panel = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
    const grid = (panel.gridPos && typeof panel.gridPos === 'object' ? panel.gridPos : {}) as Record<string, unknown>;
    const targets = Array.isArray(panel.targets) ? panel.targets : [];
    const firstTarget = (targets[0] && typeof targets[0] === 'object' ? targets[0] : {}) as Record<string, unknown>;
    const query = firstString(firstTarget.expr, firstTarget.rawSql, firstTarget.query, firstTarget.target) ?? 'Query adapter pending';
    const width = typeof grid.w === 'number' ? grid.w : 8;
    return {
      id: firstString(panel.id) ?? 'panel-' + index,
      title: firstString(panel.title) ?? 'Panel ' + (index + 1),
      type: firstString(panel.type, panel.pluginId) ?? 'panel',
      query,
      span: Math.max(1, Math.min(4, Math.round(width / 6))),
    };
  });
}

function DashboardWallboardPanel({ panel, index }: { panel: WallboardPanel; index: number }) {
  const { t } = useTranslation('shell');
  return (
    <div
      className="flex min-h-[190px] flex-col overflow-hidden rounded-lg border border-bd-1 bg-bg-1"
      style={{ gridColumn: 'span ' + panel.span }}
    >
      <div className="flex items-center gap-2 border-b border-bd-0 px-3 py-2">
        <span className={uiLabelStrongClass}>{panel.title}</span>
        <Pill className="ml-auto">{panel.type}</Pill>
      </div>
      <div className="flex flex-1 flex-col justify-between gap-3 p-4">
        <div className="grid flex-1 place-items-center rounded-md border border-dashed border-bd-0 bg-bg-2 text-center font-sans text-xs text-tx-2">
          <div>
            <div className="font-semibold text-tx-1">
              {t('pages.noc.dashboard_wallboard.panel_index', { index: String(index + 1).padStart(2, '0') })}
            </div>
            <div className="mt-1 max-w-[360px] truncate text-tx-3">{panel.query}</div>
          </div>
        </div>
        <div className="flex items-center justify-between font-sans text-xs font-semibold tracking-normal text-tx-2">
          <span>{t('pages.noc.dashboard_wallboard.render_pending')}</span>
          <span>{t('pages.noc.dashboard_wallboard.live')}</span>
        </div>
      </div>
    </div>
  );
}

function firstString(...values: unknown[]): string | undefined {
  for (const value of values) {
    if (typeof value === 'string' && value.trim().length > 0) return value;
    if (typeof value === 'number') return String(value);
  }
  return undefined;
}
