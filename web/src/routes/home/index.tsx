import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertTriangle,
  Archive,
  BellRing,
  Braces,
  ChevronDown,
  Clock3,
  Database,
  Gauge,
  HardDrive,
  LayoutDashboard,
  Plus,
  RadioTower,
  RefreshCw,
  Workflow,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as alertsApi from '@/api/alerts';
import * as auditApi from '@/api/audit';
import * as dashboardsApi from '@/api/dashboards';
import * as escalationsApi from '@/api/escalations';
import * as homeApi from '@/api/home';
import * as incidentsApi from '@/api/incidents';
import * as onboardingApi from '@/api/onboarding';
import * as pipelinesApi from '@/api/pipelines';
import * as schedulesApi from '@/api/schedules';
import * as streamsApi from '@/api/streams';
import * as teamsApi from '@/api/teams';
import {
  streamAttentionHref,
  summarizeStreamHealth,
} from '@/investigation/streamHealth';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { deriveActivationState } from '@/product/activation';
import { OverviewPage } from '@/product/templates';
import {
  ChromeButton,
  CriticalAlertBanner,
  type CriticalAlertItem,
} from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { queryStateFor } from '@/shell/query/State';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import { useAuthStore } from '@/stores/auth';
import { formatRelativeMicros } from '@/time/relative';

import { ActivationStrip } from './canvas/ActivationStrip';
import {
  formatAge,
  formatByteRate,
  formatBytesCompact,
  formatCount,
  streamExplorePath,
} from './canvas/format';
import {
  CanvasKpi,
  CanvasKpiStrip,
  CanvasSurfaceGrid,
  OperationsCanvas,
} from './canvas/layout';
import {
  HOME_RECENT_ACTIVITY_LIMIT,
  RecentActivitySection,
} from './canvas/RecentActivitySection';
import { RecentDashboardsSection } from './canvas/RecentDashboardsSection';
import {
  type HomeChartMetric,
  SystemHealthSection,
} from './canvas/SystemHealthSection';
import { TopStreamsSection } from './canvas/TopStreamsSection';
import {
  selectFeaturedOnCall,
  summarizeOnCallShift,
} from './onCall/model';
import { OnCallStatusCard } from './onCall/StatusCard';
import { QuickStartDrawer } from './QuickStartDrawer';

const HOME_WINDOWS = [
  { seconds: 24 * 60 * 60, labelKey: 'home.toolbar.window_24h' },
  { seconds: 7 * 24 * 60 * 60, labelKey: 'home.toolbar.window_7d' },
] as const;

function CompressionDetail({ overview }: { overview: homeApi.HomeOverview | undefined }) {
  const { t } = useTranslation('onboarding');
  if (!overview) return <>{t('home.loading')}</>;
  const ratio = overview.compression_savings_ratio;
  if (ratio == null) return <>{t('home.kpis.compression_pending')}</>;
  if (ratio >= 0) {
    return <>{t('home.kpis.compression_saved', { percent: (ratio * 100).toFixed(1) })}</>;
  }
  return (
    <>
      {t('home.kpis.compression_overhead', {
        percent: (Math.abs(ratio) * 100).toFixed(1),
      })}
    </>
  );
}

export function Home() {
  const { t, i18n } = useTranslation('onboarding');
  const nav = useNavigate();
  const qc = useQueryClient();
  const dashboardCreateAccess = useActionAccess({
    permission: 'dashboards.create',
  });
  const dashboardEditAccess = useActionAccess({
    permission: 'dashboards.edit',
  });
  const alertCreateAccess = useActionAccess({ permission: 'alerts.manage' });
  const streamCreateAccess = useActionAccess({ permission: 'streams.create' });
  const pipelineCreateAccess = useActionAccess({
    permission: 'pipelines.create',
  });
  const functionCreateAccess = useActionAccess({
    permission: 'functions.create',
  });
  const scheduleManageAccess = useActionAccess({
    permission: 'schedules.manage',
  });
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const currentUserId = useAuthStore(
    (state) => state.ctx?.user_id ?? '',
  );
  const users = useUsers();
  const [windowSecs, setWindowSecs] = React.useState<number>(HOME_WINDOWS[0].seconds);
  const [chartMetric, setChartMetric] = React.useState<HomeChartMetric>('intake');
  const [quickStartOpen, setQuickStartOpen] = React.useState(false);
  const [nowMicros, setNowMicros] = React.useState(
    () => Date.now() * 1000,
  );

  React.useEffect(() => {
    const timer = window.setInterval(
      () => setNowMicros(Date.now() * 1000),
      60_000,
    );
    return () => window.clearInterval(timer);
  }, []);

  const overviewQuery = useQuery({
    queryKey: ['home', 'overview', orgId, windowSecs],
    queryFn: () =>
      homeApi.overview({
        windowSecs,
        bucketCount: windowSecs > 24 * 60 * 60 ? 28 : 24,
      }),
    enabled: Boolean(orgId),
    refetchInterval: 60_000,
  });
  const sampleStatusQuery = useQuery({
    queryKey: ['onboarding', 'sample-data'],
    queryFn: onboardingApi.getSampleDataStatus,
  });
  const loadSample = useMutation({
    mutationFn: onboardingApi.loadSampleData,
    onSuccess: (result) => {
      toast.success(t('activation.load_success', { rows: result.total_rows }));
      void qc.invalidateQueries({ queryKey: ['onboarding', 'sample-data'] });
      void qc.invalidateQueries({ queryKey: ['streams'] });
      void qc.invalidateQueries({ queryKey: ['home', 'overview'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const streamsQuery = useQuery({
    queryKey: ['streams', 'list'],
    queryFn: () => streamsApi.list(200),
  });
  const runtimeQuery = useQuery({
    queryKey: ['streams', 'runtime', windowSecs],
    queryFn: () =>
      streamsApi.runtimeOverview({
        windowSecs,
        bucketCount: windowSecs > 24 * 60 * 60 ? 28 : 24,
      }),
    enabled: Boolean(orgId),
    refetchInterval: 60_000,
  });
  const dashboardsQuery = useQuery({
    queryKey: ['dashboards', 'list'],
    queryFn: () => dashboardsApi.list(),
  });
  const incidentsQuery = useQuery({
    queryKey: ['alerts', 'incidents'],
    queryFn: () =>
      incidentsApi.list({
        scope: 'all',
        window_secs: 14 * 24 * 60 * 60,
      }),
  });
  const rulesQuery = useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: () => alertsApi.list(),
  });
  const pipelinesQuery = useQuery({
    queryKey: ['pipelines', 'list'],
    queryFn: () => pipelinesApi.list(),
  });
  const activityQuery = useQuery({
    queryKey: ['audit', 'recent', HOME_RECENT_ACTIVITY_LIMIT],
    queryFn: () => auditApi.recent(HOME_RECENT_ACTIVITY_LIMIT),
  });
  const schedulesQuery = useQuery({
    queryKey: ['schedules'],
    queryFn: schedulesApi.list,
    refetchInterval: 60_000,
  });
  const escalationPoliciesQuery = useQuery({
    queryKey: ['escalation-policies'],
    queryFn: escalationsApi.list,
  });
  const teamsQuery = useQuery({
    queryKey: ['teams'],
    queryFn: teamsApi.list,
  });

  const overview = overviewQuery.data;
  const runtimeStreams = runtimeQuery.data?.streams ?? [];
  const streamHealth = summarizeStreamHealth(runtimeStreams);
  const streamHealthStatus = runtimeQuery.isError
    ? 'unknown'
    : streamHealth.status;
  const runtimeAsOf = formatRelativeMicros(
    runtimeQuery.data?.generated_at_micros,
    i18n.resolvedLanguage ?? i18n.language,
  );
  const streams = streamsQuery.data ?? [];
  const dashboards = dashboardsQuery.data ?? [];
  const incidents = React.useMemo(
    () => incidentsQuery.data ?? [],
    [incidentsQuery.data],
  );
  const rules = rulesQuery.data ?? [];
  const pipelines = pipelinesQuery.data ?? [];
  const activity = activityQuery.data ?? [];
  const escalationPolicies = React.useMemo(
    () => escalationPoliciesQuery.data ?? [],
    [escalationPoliciesQuery.data],
  );
  const schedules = React.useMemo(
    () => schedulesQuery.data ?? [],
    [schedulesQuery.data],
  );
  const teams = React.useMemo(
    () => teamsQuery.data ?? [],
    [teamsQuery.data],
  );
  const teamsById = React.useMemo(
    () => new Map(teams.map((team) => [team.id, team])),
    [teams],
  );
  const firing = incidents.filter((incident) => incident.status === 'open').length;
  const acknowledged = incidents.filter((incident) => incident.status === 'acknowledged').length;
  const activeIncidents = firing + acknowledged;

  const criticalItems: CriticalAlertItem[] = incidents
    .filter((incident) => incident.status === 'open')
    .slice(0, 5)
    .map((incident) => {
      const service = incident.affected_services?.[0];
      const age = formatAge(incident.created_at);
      return {
        id: incident.id,
        label: incident.summary || incident.id,
        meta: service ? `${service} · ${age}` : age,
      };
    });

  const activation = deriveActivationState({
    streamsCount: streams.length,
    dashboardsCount: dashboards.length,
    alertsCount: rules.length,
    pipelinesCount: pipelines.length,
    sampleDataAvailable: sampleStatusQuery.data?.loaded ?? false,
  });

  const featuredOnCall = React.useMemo(
    () =>
      selectFeaturedOnCall(
        schedules,
        currentUserId,
        nowMicros,
      ),
    [currentUserId, nowMicros, schedules],
  );
  const featuredTeamName =
    featuredOnCall?.schedule.team_id
      ? teamsById.get(featuredOnCall.schedule.team_id)?.name
      : undefined;
  const shiftOverview = React.useMemo(
    () =>
      featuredOnCall &&
      !incidentsQuery.isLoading &&
      !incidentsQuery.isError &&
      !escalationPoliciesQuery.isLoading &&
      !escalationPoliciesQuery.isError
        ? summarizeOnCallShift(
            featuredOnCall,
            incidents,
            escalationPolicies,
          )
        : null,
    [
      escalationPolicies,
      escalationPoliciesQuery.isError,
      escalationPoliciesQuery.isLoading,
      featuredOnCall,
      incidents,
      incidentsQuery.isError,
      incidentsQuery.isLoading,
    ],
  );

  React.useEffect(() => {
    if (overview && overview.intake_bytes == null && chartMetric === 'intake') {
      setChartMetric('stored');
    }
  }, [chartMetric, overview]);

  const activityState = queryStateFor({
    isLoading: activityQuery.isLoading,
    isError: activityQuery.isError,
    data: activity,
  });
  const overviewState = queryStateFor({
    isLoading: overviewQuery.isLoading,
    isError: overviewQuery.isError,
    data: overview?.streams,
  });
  const selectedWindow =
    HOME_WINDOWS.find((item) => item.seconds === windowSecs) ?? HOME_WINDOWS[0];
  const isRefreshing = [
    overviewQuery,
    runtimeQuery,
    streamsQuery,
    dashboardsQuery,
    incidentsQuery,
    rulesQuery,
    pipelinesQuery,
    activityQuery,
    schedulesQuery,
    escalationPoliciesQuery,
    teamsQuery,
  ].some((query) => query.isFetching);

  const refresh = async () => {
    await Promise.all([
      overviewQuery.refetch(),
      runtimeQuery.refetch(),
      streamsQuery.refetch(),
      dashboardsQuery.refetch(),
      incidentsQuery.refetch(),
      rulesQuery.refetch(),
      pipelinesQuery.refetch(),
      activityQuery.refetch(),
      schedulesQuery.refetch(),
      escalationPoliciesQuery.refetch(),
      teamsQuery.refetch(),
    ]);
  };

  const quickStart = () => setQuickStartOpen(true);

  return (
    <OverviewPage
      title={t('home.title')}
      subtitle={t('home.subtitle')}
      appearance="surface"
      headerCompact
      bodyClassName="gap-[12px] pb-[20px] pt-0"
      toolbar={
        <>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <ChromeButton variant="ghost" className="bg-bg-2">
                <Clock3 className="h-3.5 w-3.5" />
                {t(selectedWindow.labelKey)}
                <ChevronDown className="h-3.5 w-3.5 text-tx-3" />
              </ChromeButton>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-44">
              <DropdownMenuRadioGroup
                value={String(windowSecs)}
                onValueChange={(value) => setWindowSecs(Number(value))}
              >
                {HOME_WINDOWS.map((item) => (
                  <DropdownMenuRadioItem key={item.seconds} value={String(item.seconds)}>
                    {t(item.labelKey)}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuContent>
          </DropdownMenu>
          <ChromeButton
            variant="ghost"
            className="w-9 justify-center px-0"
            onClick={() => void refresh()}
            disabled={isRefreshing}
            aria-label={t('home.toolbar.refresh')}
            title={t('home.toolbar.refresh')}
          >
            <RefreshCw className={cn('h-3.5 w-3.5', isRefreshing && 'animate-spin')} />
          </ChromeButton>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <ChromeButton variant="primary" className="ml-1">
                <Plus className="h-3.5 w-3.5" />
                {t('home.toolbar.new')}
                <ChevronDown className="h-3.5 w-3.5" />
              </ChromeButton>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-48">
              <DropdownMenuItem
                disabled={dashboardCreateAccess.disabled}
                disabledReason={dashboardCreateAccess.reason}
                onSelect={() => nav('/dashboards/new/edit')}
              >
                <LayoutDashboard className="h-4 w-4 text-purple-soft" />
                {t('home.toolbar.new_dashboard')}
              </DropdownMenuItem>
              <DropdownMenuItem
                disabled={alertCreateAccess.disabled}
                disabledReason={alertCreateAccess.reason}
                onSelect={() => nav('/alerts/rules/new')}
              >
                <BellRing className="h-4 w-4 text-red-soft" />
                {t('home.toolbar.new_alert')}
              </DropdownMenuItem>
              <DropdownMenuItem
                disabled={streamCreateAccess.disabled}
                disabledReason={streamCreateAccess.reason}
                onSelect={() => nav('/streams?create=1')}
              >
                <Database className="h-4 w-4 text-orange-soft" />
                {t('home.toolbar.new_stream')}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                disabled={pipelineCreateAccess.disabled}
                disabledReason={pipelineCreateAccess.reason}
                onSelect={() => nav('/pipelines/new')}
              >
                <Workflow className="h-4 w-4 text-green-soft" />
                {t('home.toolbar.new_pipeline')}
              </DropdownMenuItem>
              <DropdownMenuItem
                disabled={functionCreateAccess.disabled}
                disabledReason={functionCreateAccess.reason}
                onSelect={() => nav('/functions/new')}
              >
                <Braces className="h-4 w-4 text-blue-soft" />
                {t('home.toolbar.new_function')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </>
      }
    >
      <div>
        <OperationsCanvas>
          <CanvasKpiStrip label={t('home.kpis.label')}>
          <CanvasKpi
            label={t('home.kpis.intake_status')}
            value={runtimeQuery.isLoading ? '—' : t(`home.status.${streamHealthStatus}`)}
            detail={
              overview
                ? t('home.kpis.last_received', {
                    when: formatRelativeMicros(
                      overview.last_received_at_micros,
                      i18n.resolvedLanguage ?? i18n.language,
                      overview.generated_at_micros,
                    ),
                  })
                : t('home.loading')
            }
            icon={<RadioTower className="h-4 w-4" />}
            status={streamHealthStatus}
            onClick={() => nav(`/streams?window_secs=${windowSecs}`)}
          />
          <CanvasKpi
            label={t('home.kpis.active_alerts')}
            value={incidentsQuery.isLoading ? '—' : String(activeIncidents)}
            detail={t('home.kpis.alert_detail', { firing, acknowledged })}
            icon={<BellRing className="h-4 w-4" />}
            status={firing > 0 ? 'degraded' : activeIncidents > 0 ? 'delayed' : 'healthy'}
            onClick={() => nav('/alerts')}
          />
          <CanvasKpi
            label={t('home.kpis.intake_bytes')}
            value={formatBytesCompact(overview?.intake_bytes)}
            detail={
              overview
                ? t('home.kpis.intake_detail', {
                    rate: formatByteRate(overview.intake_bytes, overview.window?.window_secs ?? 1),
                    rows: formatCount(overview.rows),
                  })
                : t('home.loading')
            }
            icon={<Archive className="h-4 w-4" />}
            onClick={() => nav('/streams')}
          />
          <CanvasKpi
            label={t('home.kpis.stored_bytes')}
            value={formatBytesCompact(overview?.stored_bytes)}
            detail={<CompressionDetail overview={overview} />}
            icon={<HardDrive className="h-4 w-4" />}
            onClick={() => nav('/streams')}
          />
          <CanvasKpi
            label={t('home.kpis.attention_streams')}
            value={runtimeQuery.isLoading ? '—' : String(streamHealth.attention)}
            detail={
              runtimeQuery.data
                ? t('home.kpis.attention_detail', {
                    total: streamHealth.total,
                    asOf: runtimeAsOf,
                  })
                : t('home.loading')
            }
            icon={<AlertTriangle className="h-4 w-4" />}
            status={
              runtimeQuery.data
                ? streamHealth.attention > 0
                  ? 'delayed'
                  : 'healthy'
                : undefined
            }
            onClick={() => nav(streamAttentionHref(windowSecs))}
          />
          <CanvasKpi
            label={t('home.kpis.active_sources')}
            value={
              runtimeQuery.isLoading
                ? '—'
                : `${streamHealth.receiving} / ${streamHealth.total}`
            }
            detail={
              runtimeQuery.data
                ? t('home.kpis.receiving_detail', {
                    inactive: streamHealth.inactive,
                    asOf: runtimeAsOf,
                  })
                : t('home.loading')
            }
            icon={<Gauge className="h-4 w-4" />}
            onClick={() => nav(`/streams?status=healthy&window_secs=${windowSecs}`)}
          />
          </CanvasKpiStrip>

          {criticalItems.length > 0 && (
            <div className="rounded-md bg-[var(--functional-surface)] p-[12px] [box-shadow:var(--shadow-functional-surface)]">
              <CriticalAlertBanner
                title={t('home.critical.title', { count: firing })}
                items={criticalItems}
                viewAllLabel={t('home.view_all')}
                onViewAll={() => nav('/alerts')}
              />
            </div>
          )}

          <CanvasSurfaceGrid className="home-canvas-primary-grid items-stretch">
            <SystemHealthSection
              overview={overview}
              runtimeOverview={runtimeQuery.data}
              state={overviewState}
              error={overviewQuery.error}
              metric={chartMetric}
              onMetricChange={setChartMetric}
              windowLabel={t(selectedWindow.labelKey)}
              onOpenStreams={() => nav('/streams')}
            />

            <aside
              aria-label={t('home.sidebar_label')}
              className="min-w-0"
              data-testid="home-primary-operational-context"
            >
              {featuredOnCall?.status === 'gap' ? (
                <OnCallStatusCard
                  surface="canvas"
                  feature={featuredOnCall}
                  teamName={featuredTeamName}
                  usersById={users.byId}
                  shiftOverview={shiftOverview}
                  nowMicros={nowMicros}
                  locale={i18n.language}
                  loading={schedulesQuery.isLoading || users.isLoading}
                  onViewSchedule={() =>
                    nav(
                      `/alerts/schedules/${encodeURIComponent(
                        featuredOnCall.schedule.id,
                      )}`,
                    )
                  }
                  onViewEscalations={() => nav('/alerts/escalations')}
                  onOpenIncidents={() => nav('/alerts/incidents')}
                  onArrange={() =>
                    nav(
                      `/alerts/schedules/${encodeURIComponent(
                        featuredOnCall.schedule.id,
                      )}?addOverride=1`,
                    )
                  }
                  arrangeDisabled={scheduleManageAccess.disabled}
                  arrangeDisabledReason={scheduleManageAccess.reason}
                />
              ) : (
                <OnCallStatusCard
                  surface="canvas"
                  feature={featuredOnCall}
                  teamName={featuredTeamName}
                  usersById={users.byId}
                  shiftOverview={shiftOverview}
                  nowMicros={nowMicros}
                  locale={i18n.language}
                  loading={schedulesQuery.isLoading || users.isLoading}
                  onViewSchedule={() =>
                    featuredOnCall
                      ? nav(
                          `/alerts/schedules/${encodeURIComponent(
                            featuredOnCall.schedule.id,
                          )}`,
                        )
                      : nav('/alerts/schedules')
                  }
                  onViewEscalations={() => nav('/alerts/escalations')}
                  onOpenIncidents={() => nav('/alerts/incidents')}
                  onArrange={() => nav('/alerts/schedules')}
                  arrangeDisabled={false}
                />
              )}
            </aside>
          </CanvasSurfaceGrid>

          <CanvasSurfaceGrid className="home-canvas-detail-grid items-stretch">
            <TopStreamsSection
              overview={overview}
              runtimeOverview={runtimeQuery.data}
              state={overviewState}
              error={overviewQuery.error}
              onOpen={(stream) => nav(streamExplorePath(stream))}
              onViewAll={() => nav('/streams')}
            />
            <RecentActivitySection
              events={activity}
              state={activityState}
              error={activityQuery.error}
              onViewAll={() => nav('/settings/audit')}
              onCreateAlert={() => nav('/alerts/rules/new')}
              createAlertDisabled={alertCreateAccess.disabled}
              createAlertDisabledReason={alertCreateAccess.reason}
            />
          </CanvasSurfaceGrid>

          <div className="space-y-[8px] bg-[var(--page-canvas)]">
            <RecentDashboardsSection
              dashboards={dashboards}
              loading={dashboardsQuery.isLoading}
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
            />
            <ActivationStrip state={activation} onOpen={quickStart} />
          </div>
        </OperationsCanvas>
      </div>
      <QuickStartDrawer
        open={quickStartOpen}
        onOpenChange={setQuickStartOpen}
        state={activation}
        onOpenStep={(to) => {
          setQuickStartOpen(false);
          nav(to);
        }}
        onLoadSample={() => loadSample.mutate()}
        loadingSample={loadSample.isPending}
      />
    </OverviewPage>
  );
}
