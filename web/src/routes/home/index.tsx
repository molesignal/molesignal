import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { streamAttentionHref } from '@/investigation/streamHealth';
import { canAccessProductPath, useProductAccess } from '@/product/access';
import { useActionAccess } from '@/product/actionAccess';
import { OverviewPage } from '@/product/templates';
import { CriticalAlertBanner } from '@/shell/chrome';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';
import { formatRelativeMicros } from '@/time/relative';

import { ActivationStrip } from './canvas/ActivationStrip';
import { streamExplorePath } from './canvas/format';
import { HeroStrip } from './canvas/HeroStrip';
import {
  CanvasColumn,
  CanvasSurfaceGrid,
  OperationsCanvas,
} from './canvas/layout';
import { RecentActivitySection } from './canvas/RecentActivitySection';
import { RecentDashboardsSection } from './canvas/RecentDashboardsSection';
import { SignalLine } from './canvas/SignalLine';
import {
  type HomeChartMetric,
  SystemHealthSection,
} from './canvas/SystemHealthSection';
import { TopStreamsSection } from './canvas/TopStreamsSection';
import { HOME_WINDOWS, HomeToolbar, homeWindowFor } from './HomeToolbar';
import { OnCallStatusCard } from './onCall/StatusCard';
import { QuickStartDrawer } from './QuickStartDrawer';
import { useHomeData } from './useHomeData';

export function Home() {
  const { t, i18n } = useTranslation('onboarding');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const nav = useNavigate();
  const dashboardCreateAccess = useActionAccess({ permission: 'dashboards.create' });
  const dashboardEditAccess = useActionAccess({ permission: 'dashboards.edit' });
  const alertCreateAccess = useActionAccess({ permission: 'alerts.manage' });
  const scheduleManageAccess = useActionAccess({ permission: 'schedules.manage' });
  const productAccess = useProductAccess();
  const canAskAgent = canAccessProductPath('/agent', productAccess);
  const [windowSecs, setWindowSecs] = React.useState<number>(HOME_WINDOWS[0].seconds);
  const [chartMetric, setChartMetric] = React.useState<HomeChartMetric>('intake');
  const [quickStartOpen, setQuickStartOpen] = React.useState(false);
  const data = useHomeData(windowSecs);
  const { overview } = data;
  const windowLabel = t(homeWindowFor(windowSecs).labelKey);

  React.useEffect(() => {
    if (overview && overview.intake_bytes == null && chartMetric === 'intake') {
      setChartMetric('stored');
    }
  }, [chartMetric, overview]);

  const lastReceived = formatRelativeMicros(
    data.fleet?.lastReceivedAtMicros ?? overview?.last_received_at_micros,
    locale,
    overview?.generated_at_micros,
  );
  const connect = () => nav('/datasource');
  const investigate = () => nav(streamAttentionHref(windowSecs));
  const schedule = data.featuredOnCall?.schedule;
  const scheduleHref = schedule
    ? `/alerts/schedules/${encodeURIComponent(schedule.id)}`
    : '/alerts/schedules';
  const hasCoverageGap = data.featuredOnCall?.status === 'gap';

  return (
    <OverviewPage
      title={t('home.title')}
      subtitle={t('home.subtitle')}
      appearance="surface"
      headerCompact
      bodyClassName="pb-[24px] pt-0"
      toolbar={
        <HomeToolbar
          windowSecs={windowSecs}
          onWindowChange={setWindowSecs}
          onRefresh={() => void data.refresh()}
          refreshing={data.isRefreshing}
          updatedAt={data.updatedAt}
        />
      }
    >
      <div>
        <OperationsCanvas>
          {data.criticalItems.length > 0 && (
            <div className="rounded-md bg-[var(--functional-surface)] p-[12px] [box-shadow:var(--shadow-functional-surface)]">
              <CriticalAlertBanner
                title={t('home.critical.title', { count: data.firing })}
                items={data.criticalItems}
                viewAllLabel={t('home.view_all')}
                onViewAll={() => nav('/alerts')}
              />
            </div>
          )}

          <HeroStrip
            fleet={data.fleet}
            fleetLoading={data.runtimeLoading}
            overview={overview}
            overviewLoading={data.overviewLoading}
            firing={data.firing}
            acknowledged={data.acknowledged}
            incidentsLoading={data.incidentsLoading}
            windowLabel={windowLabel}
            lastReceived={lastReceived}
            actions={{
              investigate,
              connect,
              viewStreams: () => nav(`/streams?window_secs=${windowSecs}`),
              retry: () => void data.refresh(),
              askAgent: canAskAgent
                ? (prompt) => useMoleAgentStore.getState().open(prompt)
                : undefined,
            }}
            heroDecoration={
              overview ? (
                <SignalLine
                  values={(overview.buckets ?? []).map(
                    (bucket) => bucket.intake_bytes ?? bucket.stored_bytes,
                  )}
                  className="pointer-events-none absolute inset-x-0 bottom-0 h-[22px] w-full opacity-60 [mask-image:linear-gradient(to_right,transparent,#000_35%)]"
                />
              ) : undefined
            }
            onOpenAlerts={() => nav('/alerts')}
            onOpenStreams={() => nav('/streams')}
          />

          <ActivationStrip
            state={data.activation}
            onOpen={() => setQuickStartOpen(true)}
            riseIndex={4}
          />

          <CanvasSurfaceGrid className="home-canvas-workspace">
            <CanvasColumn>
              <SystemHealthSection
                overview={overview}
                runtimeOverview={data.runtimeOverview}
                state={data.overviewState}
                error={data.overviewError}
                metric={chartMetric}
                onMetricChange={setChartMetric}
                windowLabel={windowLabel}
                onOpenStreams={() => nav('/streams')}
                onConnect={connect}
                onInvestigate={investigate}
                riseIndex={5}
              />
              <TopStreamsSection
                overview={overview}
                runtimeOverview={data.runtimeOverview}
                state={data.overviewState}
                error={data.overviewError}
                onOpen={(stream) => nav(streamExplorePath(stream))}
                onViewAll={() => nav('/streams')}
                onConnect={connect}
                riseIndex={6}
              />
            </CanvasColumn>

            <aside
              aria-label={t('home.sidebar_label')}
              className="home-canvas-column"
              data-testid="home-primary-operational-context"
            >
              <OnCallStatusCard
                surface="canvas"
                className="home-rise home-canvas-oncall"
                feature={data.featuredOnCall}
                teamName={data.featuredTeamName}
                usersById={data.users.byId}
                shiftOverview={data.shiftOverview}
                nowMicros={data.nowMicros}
                locale={i18n.language}
                loading={data.schedulesLoading || data.users.isLoading}
                onViewSchedule={() => nav(scheduleHref)}
                onViewEscalations={() => nav('/alerts/escalations')}
                onOpenIncidents={() => nav('/alerts/incidents')}
                onArrange={() =>
                  nav(
                    hasCoverageGap
                      ? `${scheduleHref}?addOverride=1`
                      : '/alerts/schedules',
                  )
                }
                arrangeDisabled={scheduleManageAccess.disabled}
                arrangeDisabledReason={scheduleManageAccess.reason}
              />
              <RecentActivitySection
                events={data.activity}
                state={data.activityState}
                error={data.activityError}
                onViewAll={() => nav('/settings/audit')}
                onCreateAlert={() => nav('/alerts/rules/new')}
                createAlertDisabled={alertCreateAccess.disabled}
                createAlertDisabledReason={alertCreateAccess.reason}
                riseIndex={7}
              />
            </aside>
          </CanvasSurfaceGrid>

          <RecentDashboardsSection
            dashboards={data.dashboards}
            loading={data.dashboardsLoading}
            onOpenDashboard={(id) => nav(`/dashboards/${encodeURIComponent(id)}`)}
            onAddPanels={(id) =>
              nav(`/dashboards/${encodeURIComponent(id)}/panels/new`)
            }
            onCreateDashboard={() => nav('/dashboards/new/edit')}
            createDashboardDisabled={dashboardCreateAccess.disabled}
            createDashboardDisabledReason={dashboardCreateAccess.reason}
            editDashboardDisabled={dashboardEditAccess.disabled}
            editDashboardDisabledReason={dashboardEditAccess.reason}
            onViewAll={() => nav('/dashboards')}
            riseIndex={8}
          />
        </OperationsCanvas>
      </div>
      <QuickStartDrawer
        open={quickStartOpen}
        onOpenChange={setQuickStartOpen}
        state={data.activation}
        onOpenStep={(to) => {
          setQuickStartOpen(false);
          nav(to);
        }}
        onLoadSample={() => data.loadSample.mutate()}
        loadingSample={data.loadSample.isPending}
      />
    </OverviewPage>
  );
}
