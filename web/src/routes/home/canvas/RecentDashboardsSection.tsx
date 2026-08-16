import { ChevronRight, LayoutDashboard, Plus } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { dashboardPanelCount } from '@/dashboard-engine/summary';
import { ChromeButton } from '@/shell/chrome';
import { formatRelativeMicros } from '@/time/relative';
import type { Dashboard } from '@/types/dashboard';

import { CanvasHeaderAction, CanvasSection } from './layout';

const HOME_DASHBOARD_LIMIT = 4;

export function RecentDashboardsSection({
  dashboards,
  loading,
  onOpenDashboard,
  onAddPanels,
  onCreateDashboard,
  createDashboardDisabled,
  createDashboardDisabledReason,
  editDashboardDisabled,
  editDashboardDisabledReason,
  onViewAll,
}: {
  dashboards: Dashboard[];
  loading: boolean;
  onOpenDashboard: (id: string) => void;
  onAddPanels: (id: string) => void;
  onCreateDashboard: () => void;
  createDashboardDisabled: boolean;
  createDashboardDisabledReason?: string | undefined;
  editDashboardDisabled: boolean;
  editDashboardDisabledReason?: string | undefined;
  onViewAll: () => void;
}) {
  const { t, i18n } = useTranslation('onboarding');
  const orderedDashboards = [...dashboards].sort(
    (a, b) =>
      (b.updated_at || b.created_at) - (a.updated_at || a.created_at),
  );
  const recentDashboards = orderedDashboards
    .filter((dashboard) => dashboardPanelCount(dashboard) > 0)
    .slice(0, HOME_DASHBOARD_LIMIT);
  const dashboardToBuild =
    recentDashboards.length === 0 ? orderedDashboards[0] : undefined;

  return (
    <CanvasSection
      className="home-canvas-footer-section"
      title={t('home.dashboards.title')}
      actions={
        <CanvasHeaderAction label={t('home.view_all')} onClick={onViewAll} />
      }
    >
      {loading ? (
        <div className="grid min-h-14 place-items-center font-sans text-xs text-tx-2">
          {t('home.loading')}
        </div>
      ) : recentDashboards.length === 0 ? (
        <div className="flex min-h-[52px] flex-wrap items-center gap-x-4 gap-y-2 px-4 py-1.5">
          <div className="flex min-w-0 items-center gap-3">
            <span className="grid h-7 w-7 shrink-0 place-items-center rounded-md bg-purple-dim text-purple-soft">
              <LayoutDashboard aria-hidden="true" className="h-3.5 w-3.5" />
            </span>
            <div className="min-w-0">
              <div className="font-sans text-sm font-strong text-tx-1">
                {dashboardToBuild
                  ? t('home.dashboards.empty_with_dashboards_title')
                  : t('home.dashboards.empty_title')}
              </div>
              <div className="mt-0.5 font-sans text-xs text-tx-2">
                {dashboardToBuild
                  ? t('home.dashboards.empty_with_dashboards_description')
                  : t('home.dashboards.empty_description')}
              </div>
            </div>
          </div>
          <ChromeButton
            variant="ghost"
            size="sm"
            disabled={
              dashboardToBuild
                ? editDashboardDisabled
                : createDashboardDisabled
            }
            disabledReason={
              dashboardToBuild
                ? editDashboardDisabledReason
                : createDashboardDisabledReason
            }
            onClick={() =>
              dashboardToBuild
                ? onAddPanels(dashboardToBuild.id)
                : onCreateDashboard()
            }
          >
            <Plus aria-hidden="true" className="h-3.5 w-3.5" />
            {dashboardToBuild
              ? t('home.dashboards.add_panels')
              : t('home.toolbar.new_dashboard')}
          </ChromeButton>
        </div>
      ) : (
        <ul className="px-2 pb-2">
          {recentDashboards.map((dashboard) => {
            const panelCount = dashboardPanelCount(dashboard);
            const updated = formatRelativeMicros(
              dashboard.updated_at || dashboard.created_at,
              i18n.resolvedLanguage ?? i18n.language,
            );

            return (
              <li key={dashboard.id}>
                <button
                  type="button"
                  data-testid="recent-dashboard-row"
                  onClick={() => onOpenDashboard(dashboard.id)}
                  className="group grid min-h-14 w-full grid-cols-[minmax(0,1fr)_1rem] items-center gap-3 rounded-md px-3 py-2.5 text-left transition-colors duration-fast hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none md:grid-cols-[minmax(0,1fr)_7rem_10rem_1rem]"
                >
                  <span className="flex min-w-0 items-center gap-3">
                    <span className="grid h-8 w-8 shrink-0 place-items-center rounded-md bg-purple-dim text-purple-soft transition-colors duration-fast group-hover:bg-bg-3">
                      <LayoutDashboard
                        aria-hidden="true"
                        className="h-4 w-4"
                      />
                    </span>
                    <span className="truncate font-sans text-sm font-strong text-tx-0">
                      {dashboard.title}
                    </span>
                  </span>
                  <span className="hidden text-right font-sans text-xs tabular-nums text-tx-2 md:block">
                    {t('home.dashboards.panel_count', { count: panelCount })}
                  </span>
                  <span className="hidden text-right font-sans text-xs tabular-nums text-tx-2 md:block">
                    {t('home.dashboards.updated', { when: updated })}
                  </span>
                  <ChevronRight
                    aria-hidden="true"
                    className="h-3.5 w-3.5 text-tx-3 transition-[color,transform] duration-fast group-hover:translate-x-0.5 group-hover:text-tx-1"
                  />
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </CanvasSection>
  );
}
