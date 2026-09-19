import { ArrowLeft } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Dot, Pill, uiLabelClass, uiLabelStrongClass } from '@/shell/chrome';
import { LogoMark } from '@/shell/LogoMark';
import type { Dashboard } from '@/types/dashboard';

export function DashboardWallboard({
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
    <div
      className="fixed inset-0 z-[100] flex flex-col bg-bg-0 p-6"
      data-theme="dark"
      data-palette="default"
      data-mode="noc"
    >
      <div className="flex items-center gap-3 border-b border-bd-0 pb-3">
        <LogoMark size={32} />
        <div className="min-w-0">
          <div className={uiLabelClass}>
            {t('pages.noc.dashboard_wallboard.title')}
          </div>
          <div className="truncate font-sans text-base font-strong text-tx-0">
            {dashboard.title}{' '}
            {t('pages.noc.dashboard_wallboard.subtitle_suffix')}
          </div>
        </div>
        <div className="ml-auto">
          <div className={`text-right ${uiLabelClass}`}>{date}</div>
          <div className="flex items-baseline">
            <span className="font-sans text-[56px] font-display-strong leading-none tracking-tight text-tx-0">
              {time}
            </span>
            <span className="ml-2 font-sans text-xs text-tx-2">
              {t('pages.noc.utc_label')}
            </span>
          </div>
        </div>
      </div>

      <div className="my-3 grid flex-1 auto-rows-fr grid-cols-4 gap-3 overflow-hidden">
        {panels.length > 0 ? (
          panels
            .slice(0, 12)
            .map((panel, index) => (
              <DashboardWallboardPanel
                key={panel.id}
                panel={panel}
                index={index}
              />
            ))
        ) : (
          <div className="col-span-4 grid place-items-center rounded-lg border border-dashed border-bd-1 bg-bg-1 font-sans text-sm text-tx-2">
            {t('pages.noc.dashboard_wallboard.no_panels')}
          </div>
        )}
      </div>

      <div className="flex items-center gap-3 overflow-hidden border-t border-bd-0 pt-2 font-sans text-xs font-semibold tracking-normal text-tx-2">
        <span>
          {t('pages.noc.dashboard_wallboard.panels_count', {
            count: panels.length,
          })}
        </span>
        <span>·</span>
        <span>
          {t('pages.noc.dashboard_wallboard.version', {
            version: dashboard.version,
          })}
        </span>
        <span>·</span>
        <span>{dashboard.uid}</span>
        <span className="ml-auto flex items-center gap-3">
          <button
            type="button"
            onClick={onReturn}
            className="inline-flex h-8 items-center gap-1.5 rounded-md border border-bd-1 bg-transparent px-2.5 text-tx-1 transition-colors hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none"
          >
            <ArrowLeft className="h-3.5 w-3.5" />
            {t('pages.noc.return_console')}
          </button>
          <span>{t('pages.noc.esc_to_exit')}</span>
          <span className="flex items-center gap-1">
            <Dot tone="green" /> {t('pages.noc.status_bar.live')}
          </span>
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
  const rawPanels = Array.isArray(
    (dashboard.model as { panels?: unknown[] }).panels,
  )
    ? (dashboard.model as { panels: unknown[] }).panels
    : [];
  return rawPanels.map((raw, index) => {
    const panel = (raw && typeof raw === 'object' ? raw : {}) as Record<
      string,
      unknown
    >;
    const grid = (panel.gridPos && typeof panel.gridPos === 'object'
      ? panel.gridPos
      : {}) as Record<string, unknown>;
    const targets = Array.isArray(panel.targets) ? panel.targets : [];
    const firstTarget = (targets[0] && typeof targets[0] === 'object'
      ? targets[0]
      : {}) as Record<string, unknown>;
    const query =
      firstString(
        firstTarget.expr,
        firstTarget.rawSql,
        firstTarget.query,
        firstTarget.target,
      ) ?? 'Query adapter pending';
    const width = typeof grid.w === 'number' ? grid.w : 8;
    return {
      id: firstString(panel.id) ?? `panel-${index}`,
      title: firstString(panel.title) ?? `Panel ${index + 1}`,
      type: firstString(panel.type, panel.pluginId) ?? 'panel',
      query,
      span: Math.max(1, Math.min(4, Math.round(width / 6))),
    };
  });
}

function DashboardWallboardPanel({
  panel,
  index,
}: {
  panel: WallboardPanel;
  index: number;
}) {
  const { t } = useTranslation('shell');
  return (
    <div
      className="flex min-h-[190px] flex-col overflow-hidden rounded-lg border border-bd-1 bg-bg-1"
      style={{ gridColumn: `span ${panel.span}` }}
    >
      <div className="flex items-center gap-2 border-b border-bd-0 px-3 py-2">
        <span className={uiLabelStrongClass}>{panel.title}</span>
        <Pill className="ml-auto">{panel.type}</Pill>
      </div>
      <div className="flex flex-1 flex-col justify-between gap-3 p-4">
        <div className="grid flex-1 place-items-center rounded-md border border-dashed border-bd-0 bg-bg-2 text-center font-sans text-xs text-tx-2">
          <div>
            <div className="font-semibold text-tx-1">
              {t('pages.noc.dashboard_wallboard.panel_index', {
                index: String(index + 1).padStart(2, '0'),
              })}
            </div>
            <div className="mt-1 max-w-[360px] truncate text-tx-2">
              {panel.query}
            </div>
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
