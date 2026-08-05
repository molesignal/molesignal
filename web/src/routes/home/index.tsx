import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Activity,
  AlertTriangle,
  Archive,
  BarChart3,
  BellRing,
  Braces,
  ChevronDown,
  ChevronRight,
  Clock3,
  Database,
  Gauge,
  HardDrive,
  LayoutDashboard,
  Plus,
  RadioTower,
  RefreshCw,
  Sparkles,
  Workflow,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as alertsApi from '@/api/alerts';
import * as auditApi from '@/api/audit';
import * as dashboardsApi from '@/api/dashboards';
import * as escalationsApi from '@/api/escalations';
import * as functionsApi from '@/api/functions';
import * as homeApi from '@/api/home';
import * as incidentsApi from '@/api/incidents';
import * as onboardingApi from '@/api/onboarding';
import * as pipelinesApi from '@/api/pipelines';
import * as schedulesApi from '@/api/schedules';
import * as streamsApi from '@/api/streams';
import * as teamsApi from '@/api/teams';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { deriveActivationState } from '@/product/activation';
import { OverviewPage } from '@/product/templates';
import {
  Card,
  CardBody,
  CardHeader,
  cardTextActionClass,
  ChromeButton,
  CriticalAlertBanner,
  DataTable,
  Dot,
  Pill,
  type CriticalAlertItem,
  type PillTone,
  Td,
  Th,
  Tr,
  uiLabelClass,
  uiLabelStrongClass,
} from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { QueryState, queryStateFor } from '@/shell/query/State';
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
import type { Dashboard } from '@/types/dashboard';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import {
  selectFeaturedOnCall,
  summarizeOnCallShift,
} from './onCall/model';
import { OnCallStatusCard } from './onCall/StatusCard';
import { QuickStartDrawer } from './QuickStartDrawer';
import {
  calculateHomeStreamRowCount,
  DEFAULT_HOME_STREAM_ROWS,
  shouldFillHomeStreamViewport,
} from './streamRows';

const HOME_WINDOWS = [
  { seconds: 24 * 60 * 60, labelKey: 'home.toolbar.window_24h' },
  { seconds: 7 * 24 * 60 * 60, labelKey: 'home.toolbar.window_7d' },
] as const;

const HOME_RECENT_ACTIVITY_LIMIT = 8;
const HOME_PRIMARY_PANEL_HEIGHT_CLASS = 'xl:h-[340px] xl:flex-none';

type ChartMetric = 'ingested' | 'stored' | 'rows';

const STATUS_TONE: Record<homeApi.HomeHealthStatus, PillTone> = {
  healthy: 'green',
  degraded: 'red',
  delayed: 'yellow',
  no_data: 'dim',
  unknown: 'dim',
};

const STATUS_DOT: Record<
  homeApi.HomeHealthStatus,
  'green' | 'red' | 'yellow' | 'dim'
> = {
  healthy: 'green',
  degraded: 'red',
  delayed: 'yellow',
  no_data: 'dim',
  unknown: 'dim',
};

const STREAM_TONE: Record<
  homeApi.HomeStreamOverview['stream_type'],
  PillTone
> = {
  logs: 'orange',
  metrics: 'blue',
  traces: 'green',
  profiles: 'purple',
};

/** Humanized "firing for" age from a microsecond epoch. */
function formatAge(createdMicros: number | undefined): string {
  if (!createdMicros) return '—';
  const ms = Date.now() - Math.floor(createdMicros / 1000);
  if (ms <= 0) return '0m';
  const mins = Math.floor(ms / 60_000);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ${mins % 60}m`;
  return `${Math.floor(hrs / 24)}d ${hrs % 24}h`;
}

function formatBytes(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  const abs = Math.abs(value);
  if (abs < 1024) return `${Math.round(value)} B`;
  if (abs < 1024 ** 2) return `${(value / 1024).toFixed(1)} KiB`;
  if (abs < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MiB`;
  if (abs < 1024 ** 4) return `${(value / 1024 ** 3).toFixed(2)} GiB`;
  return `${(value / 1024 ** 4).toFixed(2)} TiB`;
}

function formatBytesCompact(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  const abs = Math.abs(value);
  if (abs < 1024) return `${Math.round(value)} B`;
  if (abs < 1024 ** 2) return `${Math.round(value / 1024)} KiB`;
  if (abs < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MiB`;
  if (abs < 1024 ** 4) return `${(value / 1024 ** 3).toFixed(1)} GiB`;
  return `${(value / 1024 ** 4).toFixed(1)} TiB`;
}

function formatCount(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  const abs = Math.abs(value);
  if (abs < 1_000) return `${Math.round(value)}`;
  if (abs < 1_000_000) return `${(value / 1_000).toFixed(1)}K`;
  if (abs < 1_000_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  return `${(value / 1_000_000_000).toFixed(2)}B`;
}

function formatEventRate(rows: number, windowSecs: number): string {
  if (rows <= 0 || windowSecs <= 0) return '—';
  const perSecond = rows / windowSecs;
  if (perSecond >= 1) return `${formatCount(perSecond)}/s`;
  const perMinute = perSecond * 60;
  if (perMinute >= 1) return `${formatCount(perMinute)}/min`;
  return `${formatCount(perMinute * 60)}/h`;
}

function formatByteRate(bytes: number | null | undefined, windowSecs: number): string {
  if (bytes == null || bytes <= 0 || windowSecs <= 0) return '—';
  return `${formatBytes(bytes / (windowSecs / 3600))}/h`;
}

function dashboardPanelCount(dashboard: Dashboard): number {
  const panels = dashboard.model.panels;
  return Array.isArray(panels) ? panels.length : 0;
}

function streamExplorePath(stream: homeApi.HomeStreamOverview): string {
  const name = encodeURIComponent(stream.name);
  if (stream.stream_type === 'logs') return `/logs?stream=${name}`;
  if (stream.stream_type === 'metrics') return `/metrics?metric=${name}`;
  if (stream.stream_type === 'traces') return `/traces?stream=${name}`;
  return `/streams/${encodeURIComponent(stream.id)}`;
}

function auditTarget(event: auditApi.AuditEvent): string {
  for (const key of ['name', 'title', 'summary', 'email']) {
    const value = event.payload[key];
    if (typeof value === 'string' && value.trim()) return value;
  }
  return event.target_id ?? event.target_kind ?? event.actor_kind;
}

function humanizeAction(action: string): string {
  return action
    .replace(/[._:/-]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

function CardHeaderAction({
  label,
  onClick,
}: {
  label: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(cardTextActionClass, '-mr-1')}
    >
      {label}
      <ChevronRight aria-hidden="true" className="h-3 w-3" />
    </button>
  );
}

export function Home() {
  const { t, i18n } = useTranslation('onboarding');
  const nav = useNavigate();
  const qc = useQueryClient();
  const dashboardCreateAccess = useActionAccess({
    permission: 'dashboards.create',
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
  const [chartMetric, setChartMetric] = React.useState<ChartMetric>('ingested');
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
  const functionsQuery = useQuery({
    queryKey: ['functions', 'list'],
    queryFn: () => functionsApi.list(),
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
  const streams = streamsQuery.data ?? [];
  const dashboards = dashboardsQuery.data ?? [];
  const incidents = React.useMemo(
    () => incidentsQuery.data ?? [],
    [incidentsQuery.data],
  );
  const rules = rulesQuery.data ?? [];
  const pipelines = pipelinesQuery.data ?? [];
  const functions = functionsQuery.data ?? [];
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
      const service = incident.affected_services[0];
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
    if (overview && overview.ingested_bytes == null && chartMetric === 'ingested') {
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
    streamsQuery,
    dashboardsQuery,
    incidentsQuery,
    rulesQuery,
    pipelinesQuery,
    functionsQuery,
    activityQuery,
    schedulesQuery,
    escalationPoliciesQuery,
    teamsQuery,
  ].some((query) => query.isFetching);

  const refresh = async () => {
    await Promise.all([
      overviewQuery.refetch(),
      streamsQuery.refetch(),
      dashboardsQuery.refetch(),
      incidentsQuery.refetch(),
      rulesQuery.refetch(),
      pipelinesQuery.refetch(),
      functionsQuery.refetch(),
      activityQuery.refetch(),
      schedulesQuery.refetch(),
      escalationPoliciesQuery.refetch(),
      teamsQuery.refetch(),
    ]);
  };

  const quickStart = () => setQuickStartOpen(true);

  const latestDashboards = [...dashboards]
    .sort((a, b) => (b.updated_at || b.created_at) - (a.updated_at || a.created_at))
    .slice(0, 4);

  return (
    <OverviewPage
      title={t('home.title')}
      subtitle={t('home.subtitle')}
      bodyClassName="gap-4"
      toolbar={
        <>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <ChromeButton>
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
          <ChromeButton onClick={() => void refresh()} disabled={isRefreshing}>
            <RefreshCw className={cn('h-3.5 w-3.5', isRefreshing && 'animate-spin')} />
            {t('home.toolbar.refresh')}
          </ChromeButton>
          <ChromeButton onClick={quickStart}>
            <Sparkles className="h-3.5 w-3.5" />
            {t('home.toolbar.quick_start')}
          </ChromeButton>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <ChromeButton variant="primary">
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
      <div className="space-y-4">
        <section
          aria-label={t('home.kpis.label')}
          className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-6"
        >
          <HomeKpiCard
            label={t('home.kpis.ingest_status')}
            value={overview ? t(`home.status.${overview.ingest_status}`) : '—'}
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
            status={overview?.ingest_status}
            onClick={() => nav('/streams')}
          />
          <HomeKpiCard
            label={t('home.kpis.active_alerts')}
            value={incidentsQuery.isLoading ? '—' : String(activeIncidents)}
            detail={t('home.kpis.alert_detail', { firing, acknowledged })}
            icon={<BellRing className="h-4 w-4" />}
            status={firing > 0 ? 'degraded' : activeIncidents > 0 ? 'delayed' : 'healthy'}
            onClick={() => nav('/alerts')}
          />
          <HomeKpiCard
            label={t('home.kpis.ingested_bytes')}
            value={formatBytesCompact(overview?.ingested_bytes)}
            detail={
              overview
                ? t('home.kpis.ingest_detail', {
                    rate: formatByteRate(overview.ingested_bytes, overview.window.window_secs),
                    rows: formatCount(overview.rows),
                  })
                : t('home.loading')
            }
            icon={<Archive className="h-4 w-4" />}
            onClick={() => nav('/streams')}
          />
          <HomeKpiCard
            label={t('home.kpis.stored_bytes')}
            value={formatBytesCompact(overview?.stored_bytes)}
            detail={<CompressionDetail overview={overview} />}
            icon={<HardDrive className="h-4 w-4" />}
            onClick={() => nav('/streams')}
          />
          <HomeKpiCard
            label={t('home.kpis.attention_streams')}
            value={overview ? String(overview.attention_streams) : '—'}
            detail={
              overview
                ? t('home.kpis.attention_detail', { total: overview.total_streams })
                : t('home.loading')
            }
            icon={<AlertTriangle className="h-4 w-4" />}
            status={
              overview
                ? overview.attention_streams > 0
                  ? 'delayed'
                  : 'healthy'
                : undefined
            }
            onClick={() => nav('/streams')}
          />
          <HomeKpiCard
            label={t('home.kpis.active_sources')}
            value={
              overview ? `${overview.active_streams} / ${overview.total_streams}` : '—'
            }
            detail={
              overview
                ? t('home.kpis.probe_detail', {
                    succeeded: overview.stats_probe.succeeded,
                    total: overview.stats_probe.total,
                  })
                : t('home.loading')
            }
            icon={<Gauge className="h-4 w-4" />}
            onClick={() => nav('/streams')}
          />
        </section>

        <CriticalAlertBanner
          title={t('home.critical.title', { count: firing })}
          items={criticalItems}
          viewAllLabel={t('home.view_all')}
          onViewAll={() => nav('/alerts')}
        />

        <div className="grid grid-cols-1 items-stretch gap-4 xl:grid-cols-12">
          <div className="flex min-h-0 flex-col gap-4 xl:col-span-8">
            <SystemHealthOverview
              className={HOME_PRIMARY_PANEL_HEIGHT_CLASS}
              overview={overview}
              state={overviewState}
              error={overviewQuery.error}
              metric={chartMetric}
              onMetricChange={setChartMetric}
              windowLabel={t(selectedWindow.labelKey)}
              onOpenStreams={() => nav('/streams')}
            />

            <TopStreams
              className="flex-1"
              overview={overview}
              state={overviewState}
              error={overviewQuery.error}
              onOpen={(stream) => nav(streamExplorePath(stream))}
              onViewAll={() => nav('/streams')}
            />
          </div>

          <aside
            aria-label={t('home.sidebar_label')}
            className="flex min-w-0 flex-col gap-4 xl:col-span-4"
            data-testid="home-primary-operational-context"
          >
            {featuredOnCall?.status === 'gap' ? (
              <OnCallStatusCard
                className={HOME_PRIMARY_PANEL_HEIGHT_CLASS}
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
                className={HOME_PRIMARY_PANEL_HEIGHT_CLASS}
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

            <RecentActivity
              className="min-h-[220px] flex-1"
              events={activity}
              state={activityState}
              error={activityQuery.error}
              onViewAll={() => nav('/settings/audit')}
              onCreateAlert={() => nav('/alerts/rules/new')}
              createAlertDisabled={alertCreateAccess.disabled}
              createAlertDisabledReason={alertCreateAccess.reason}
            />
          </aside>
        </div>

        <DashboardAndResources
          dashboards={latestDashboards}
          counts={{
            dashboards: dashboards.length,
            rules: rules.length,
            pipelines: pipelines.length,
            functions: functions.length,
          }}
          loading={dashboardsQuery.isLoading}
          onOpenDashboard={(id) => nav(`/dashboards/${encodeURIComponent(id)}`)}
          onCreateDashboard={() => nav('/dashboards/new/edit')}
          createDashboardDisabled={dashboardCreateAccess.disabled}
          createDashboardDisabledReason={dashboardCreateAccess.reason}
          onOpenResource={(path) => nav(path)}
        />
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

function HomeKpiCard({
  label,
  value,
  detail,
  icon,
  status,
  onClick,
}: {
  label: string;
  value: React.ReactNode;
  detail: React.ReactNode;
  icon: React.ReactNode;
  status?: homeApi.HomeHealthStatus | undefined;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="group min-h-[126px] rounded-lg border border-bd-0 bg-bg-1 p-4 text-left transition-colors duration-fast hover:border-bd-1 hover:bg-bg-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo 2xl:p-5"
    >
      <div className="flex items-center justify-between gap-3">
        <span className={uiLabelClass}>{label}</span>
        <span
          className={cn(
            'grid h-8 w-8 place-items-center rounded-md bg-bg-3 text-tx-2',
            status === 'healthy' && 'bg-green-dim text-green-soft',
            status === 'degraded' && 'bg-red-dim text-red-soft',
            status === 'delayed' && 'bg-yellow-dim text-yellow-soft',
          )}
        >
          {icon}
        </span>
      </div>
      <div className="mt-3 whitespace-nowrap font-sans text-2xl font-display-strong leading-none tracking-[-0.025em] text-tx-0 2xl:text-[28px]">
        {value}
      </div>
      <div className="mt-2 font-sans text-xs leading-snug text-tx-2">{detail}</div>
    </button>
  );
}

function CompressionDetail({ overview }: { overview: homeApi.HomeOverview | undefined }) {
  const { t } = useTranslation('onboarding');
  if (!overview) return <>{t('home.loading')}</>;
  const ratio = overview.compression_savings_ratio;
  if (ratio == null) return <>{t('home.kpis.compression_pending')}</>;
  if (ratio >= 0) {
    return <>{t('home.kpis.compression_saved', { percent: (ratio * 100).toFixed(1) })}</>;
  }
  return <>{t('home.kpis.compression_overhead', { percent: (Math.abs(ratio) * 100).toFixed(1) })}</>;
}

function SystemHealthOverview({
  overview,
  state,
  error,
  metric,
  onMetricChange,
  windowLabel,
  onOpenStreams,
  className,
}: {
  overview: homeApi.HomeOverview | undefined;
  state: ReturnType<typeof queryStateFor>;
  error: unknown;
  metric: ChartMetric;
  onMetricChange: (metric: ChartMetric) => void;
  windowLabel: string;
  onOpenStreams: () => void;
  className?: string;
}) {
  const { t, i18n } = useTranslation('onboarding');
  const metricOptions: Array<{ id: ChartMetric; label: string }> = [
    { id: 'ingested', label: t('home.health.metrics.ingested') },
    { id: 'stored', label: t('home.health.metrics.stored') },
    { id: 'rows', label: t('home.health.metrics.events') },
  ];
  const chartData =
    overview?.buckets.map((bucket) => {
      if (metric === 'ingested') return bucket.ingested_bytes ?? 0;
      if (metric === 'stored') return bucket.stored_bytes;
      return bucket.rows;
    }) ?? [];
  const chartTotal =
    metric === 'ingested'
      ? overview?.ingested_bytes
      : metric === 'stored'
        ? overview?.stored_bytes
        : overview?.rows;
  const chartTotalLabel =
    metric === 'rows' ? formatCount(chartTotal) : formatBytes(chartTotal);
  const hasChartData = chartData.some((value) => value > 0);
  const timestamps =
    overview?.buckets.map((bucket) =>
      Math.round((bucket.start_micros + bucket.end_micros) / 2),
    ) ?? [];

  return (
    <Card
      className={cn('overflow-hidden', className)}
      bodyClassName="flex h-full min-h-0 flex-col"
    >
      <CardHeader
        className="shrink-0"
        title={
          <span className="flex items-center gap-2">
            <Activity className="h-4 w-4 text-indigo-soft" />
            {t('home.health.title')}
          </span>
        }
        actions={<span className="font-sans text-xs text-tx-2">{windowLabel}</span>}
      />
      {state ? (
        <div className="min-h-[300px] xl:min-h-0 xl:flex-1">
          <QueryState
            state={state}
            error={error}
            emptyLabel={t('home.health.empty')}
          />
        </div>
      ) : (
        <CardBody className="grid min-h-[300px] gap-5 p-5 xl:min-h-0 xl:flex-1 lg:grid-cols-[minmax(0,1fr)_270px]">
          <div className="min-w-0">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div>
                <div className={uiLabelClass}>{t('home.health.total')}</div>
                <div className="mt-1.5 font-sans text-2xl font-display-strong tracking-[-0.02em] text-tx-0">
                  {chartTotalLabel}
                </div>
              </div>
              <div className="flex rounded-md border border-bd-0 bg-bg-2 p-0.5">
                {metricOptions.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    disabled={item.id === 'ingested' && overview?.ingested_bytes == null}
                    aria-pressed={metric === item.id}
                    onClick={() => onMetricChange(item.id)}
                    className={cn(
                      'rounded px-2.5 py-1.5 font-sans text-xs font-strong transition-colors disabled:cursor-not-allowed disabled:opacity-40',
                      metric === item.id
                        ? 'bg-bg-4 text-tx-0'
                        : 'text-tx-2 hover:text-tx-0',
                    )}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            </div>
            <div className="mt-4 min-h-[178px]">
              {hasChartData ? (
                <TimeSeriesChart
                  series={[
                    {
                      name: metricOptions.find((item) => item.id === metric)?.label ?? metric,
                      color:
                        metric === 'ingested'
                          ? 'var(--chart-1)'
                          : metric === 'stored'
                            ? 'var(--chart-2)'
                            : 'var(--chart-7)',
                      data: chartData,
                      timestamps,
                      unit: metric === 'rows' ? 'events' : 'bytes',
                    },
                  ]}
                  xDomain={[
                    overview?.window.start_micros ?? 0,
                    overview?.window.end_micros ?? 1,
                  ]}
                  height={178}
                  showLegend={false}
                  options={{ drawStyle: 'bar', compactAxes: true }}
                />
              ) : (
                <div className="grid h-[178px] place-items-center rounded-md border border-dashed border-bd-1 bg-bg-2/40">
                  <div className="text-center">
                    <BarChart3 className="mx-auto h-5 w-5 text-tx-3" />
                    <div className="mt-2 font-sans text-xs text-tx-2">
                      {t('home.health.no_window_data')}
                    </div>
                  </div>
                </div>
              )}
            </div>
          </div>
          <div className="min-w-0 border-t border-bd-0 pt-4 lg:border-l lg:border-t-0 lg:pl-5 lg:pt-0">
            <div className="flex items-center justify-between">
              <span className={uiLabelStrongClass}>{t('home.health.signals')}</span>
              <button
                type="button"
                onClick={onOpenStreams}
                className="font-sans text-xs font-strong text-blue-soft hover:text-tx-0"
              >
                {t('home.view_all')}
              </button>
            </div>
            <div className="mt-2 divide-y divide-bd-0">
              {overview?.signals.map((signal) => (
                <div
                  key={signal.stream_type}
                  className="flex min-h-[53px] items-center gap-3 py-2.5"
                >
                  <Dot tone={STATUS_DOT[signal.status]} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center justify-between gap-2">
                      <span className="font-sans text-sm font-strong capitalize text-tx-0">
                        {signal.stream_type}
                      </span>
                      <span className="font-sans text-xs text-tx-2">
                        {formatEventRate(signal.rows, overview.window.window_secs)}
                      </span>
                    </div>
                    <div className="mt-0.5 flex items-center justify-between gap-2 font-sans text-xs text-tx-3">
                      <span>{t(`home.status.${signal.status}`)}</span>
                      <span>
                        {formatRelativeMicros(
                          signal.last_received_at_micros,
                          i18n.resolvedLanguage ?? i18n.language,
                          overview.generated_at_micros,
                        )}
                      </span>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </CardBody>
      )}
    </Card>
  );
}

function TopStreams({
  overview,
  state,
  error,
  onOpen,
  onViewAll,
  className,
}: {
  overview: homeApi.HomeOverview | undefined;
  state: ReturnType<typeof queryStateFor>;
  error: unknown;
  onOpen: (stream: homeApi.HomeStreamOverview) => void;
  onViewAll: () => void;
  className?: string;
}) {
  const { t, i18n } = useTranslation('onboarding');
  const streams = overview?.streams ?? [];
  const overviewWindowSecs = overview?.window.window_secs ?? 1;
  const generatedAtMicros = overview?.generated_at_micros;
  const tableViewportRef = React.useRef<HTMLDivElement>(null);
  const [visibleRowCount, setVisibleRowCount] = React.useState(() =>
    Math.min(streams.length, DEFAULT_HOME_STREAM_ROWS),
  );
  const [fillTableHeight, setFillTableHeight] = React.useState(false);

  React.useLayoutEffect(() => {
    const viewport = tableViewportRef.current;
    setVisibleRowCount((current) => {
      const next = Math.min(
        streams.length,
        current || DEFAULT_HOME_STREAM_ROWS,
      );
      return current === next ? current : next;
    });
    if (!viewport || streams.length === 0) {
      setFillTableHeight(false);
      return;
    }

    let animationFrame = 0;
    const measure = () => {
      window.cancelAnimationFrame(animationFrame);
      animationFrame = window.requestAnimationFrame(() => {
        const header = viewport.querySelector<HTMLTableSectionElement>('thead');
        const firstRow = viewport.querySelector<HTMLTableRowElement>('tbody tr');
        const headerHeight = header?.getBoundingClientRect().height ?? 0;
        const rowHeight = firstRow?.getBoundingClientRect().height ?? 0;
        const nextCount = calculateHomeStreamRowCount({
          viewportHeight: viewport.clientHeight,
          headerHeight,
          rowHeight,
          totalRows: streams.length,
        });
        setVisibleRowCount((current) =>
          current === nextCount ? current : nextCount,
        );
        const shouldFill = shouldFillHomeStreamViewport({
          viewportHeight: viewport.clientHeight,
          headerHeight,
          rowHeight,
          visibleRows: nextCount,
        });
        setFillTableHeight((current) =>
          current === shouldFill ? current : shouldFill,
        );
      });
    };

    measure();
    if (typeof ResizeObserver === 'undefined') {
      return () => window.cancelAnimationFrame(animationFrame);
    }

    const observer = new ResizeObserver(measure);
    observer.observe(viewport);
    const header = viewport.querySelector<HTMLTableSectionElement>('thead');
    const firstRow = viewport.querySelector<HTMLTableRowElement>('tbody tr');
    if (header) observer.observe(header);
    if (firstRow) observer.observe(firstRow);

    return () => {
      observer.disconnect();
      window.cancelAnimationFrame(animationFrame);
    };
  }, [streams.length]);

  return (
    <Card
      className={cn('min-h-[310px] overflow-hidden', className)}
      bodyClassName="flex h-full min-h-0 flex-col"
    >
      <CardHeader
        title={t('home.streams.title')}
        actions={
          <CardHeaderAction label={t('home.view_all')} onClick={onViewAll} />
        }
      />
      {state ? (
        <div className="min-h-[210px] flex-1">
          <QueryState state={state} error={error} emptyLabel={t('home.streams.empty')} />
        </div>
      ) : (
        <div
          ref={tableViewportRef}
          className={cn(
            'min-h-0 flex-1 overflow-hidden',
            fillTableHeight && '[&>div]:h-full',
          )}
          data-testid="home-top-streams-viewport"
        >
          <DataTable
            className={cn(
              'min-w-[620px]',
              fillTableHeight && 'h-full',
            )}
          >
            <thead>
              <tr>
                <Th>{t('home.streams.columns.stream')}</Th>
                <Th>{t('home.streams.columns.type')}</Th>
                <Th>{t('home.streams.columns.status')}</Th>
                <Th>{t('home.streams.columns.rate')}</Th>
                <Th>{t('home.streams.columns.last_received')}</Th>
                <Th className="w-16 whitespace-nowrap text-right">
                  {t('home.streams.columns.action')}
                </Th>
              </tr>
            </thead>
            <tbody>
              {streams.slice(0, visibleRowCount).map((stream) => (
                <Tr key={stream.id} onClick={() => onOpen(stream)}>
                  <Td className="font-strong text-tx-0">{stream.name}</Td>
                  <Td>
                    <Pill tone={STREAM_TONE[stream.stream_type]}>{stream.stream_type}</Pill>
                  </Td>
                  <Td>
                    <Pill tone={STATUS_TONE[stream.status]}>
                      <Dot tone={STATUS_DOT[stream.status]} />
                      {t(`home.status.${stream.status}`)}
                    </Pill>
                  </Td>
                  <Td className="font-mono text-xs">
                    {formatEventRate(stream.rows, overviewWindowSecs)}
                  </Td>
                  <Td className="text-tx-2">
                    {formatRelativeMicros(
                      stream.last_received_at_micros,
                      i18n.resolvedLanguage ?? i18n.language,
                      generatedAtMicros,
                    )}
                  </Td>
                  <Td className="text-right">
                    <ChevronRight className="ml-auto h-4 w-4 text-tx-3" />
                  </Td>
                </Tr>
              ))}
            </tbody>
          </DataTable>
        </div>
      )}
    </Card>
  );
}

function RecentActivity({
  events,
  state,
  error,
  onViewAll,
  onCreateAlert,
  createAlertDisabled,
  createAlertDisabledReason,
  className,
}: {
  events: auditApi.AuditEvent[];
  state: ReturnType<typeof queryStateFor>;
  error: unknown;
  onViewAll: () => void;
  onCreateAlert: () => void;
  createAlertDisabled: boolean;
  createAlertDisabledReason?: string | undefined;
  className?: string;
}) {
  const { t, i18n } = useTranslation('onboarding');
  return (
    <Card
      className={cn('overflow-hidden', className)}
      bodyClassName="flex h-full min-h-0 flex-col"
    >
      <CardHeader
        title={t('home.activity.title')}
        actions={
          <CardHeaderAction label={t('home.view_all')} onClick={onViewAll} />
        }
      />
      {state === 'empty' ? (
        <div className="grid min-h-[158px] place-items-center px-5 py-4 text-center">
          <div>
            <Activity className="mx-auto h-5 w-5 text-tx-3" />
            <div className="mt-2 font-sans text-sm font-strong text-tx-1">
              {t('home.activity.empty_title')}
            </div>
            <p className="mx-auto mt-1 max-w-[260px] font-sans text-xs leading-relaxed text-tx-2">
              {t('home.activity.empty_description')}
            </p>
            <ChromeButton
              size="sm"
              className="mt-3"
              disabled={createAlertDisabled}
              disabledReason={createAlertDisabledReason}
              onClick={onCreateAlert}
            >
              <Plus className="h-3 w-3" />
              {t('home.activity.create_alert')}
            </ChromeButton>
          </div>
        </div>
      ) : state ? (
        <div className="min-h-[158px]">
          <QueryState state={state} error={error} emptyLabel={t('home.activity.empty_title')} />
        </div>
      ) : (
        <ol className="relative min-h-0 flex-1 overflow-y-auto px-4 py-2">
          {events.slice(0, HOME_RECENT_ACTIVITY_LIMIT).map((event, index) => (
            <li key={event.id} className="relative flex min-h-[42px] gap-3 py-2">
              {index < Math.min(events.length, HOME_RECENT_ACTIVITY_LIMIT) - 1 && (
                <span
                  aria-hidden="true"
                  className="pointer-events-none absolute inset-y-0 left-0 w-1.5"
                >
                  <span className="absolute -bottom-3 left-1/2 top-[1.375rem] w-px -translate-x-1/2 bg-bd-1" />
                </span>
              )}
              <Dot
                tone="indigo"
                className="relative z-10 mt-1.5 ring-2 ring-bg-1"
              />
              <div className="min-w-0 flex-1">
                <div className="truncate font-sans text-xs font-strong text-tx-0">
                  {humanizeAction(event.action)}
                </div>
                <div className="mt-0.5 flex min-w-0 items-center gap-2 font-sans text-xs text-tx-2">
                  <span className="min-w-0 flex-1 truncate">{auditTarget(event)}</span>
                  <span className="shrink-0 text-tx-3">
                    {formatRelativeMicros(
                      event.ts_micros,
                      i18n.resolvedLanguage ?? i18n.language,
                    )}
                  </span>
                </div>
              </div>
            </li>
          ))}
        </ol>
      )}
    </Card>
  );
}

function DashboardAndResources({
  dashboards,
  counts,
  loading,
  onOpenDashboard,
  onCreateDashboard,
  createDashboardDisabled,
  createDashboardDisabledReason,
  onOpenResource,
}: {
  dashboards: Dashboard[];
  counts: { dashboards: number; rules: number; pipelines: number; functions: number };
  loading: boolean;
  onOpenDashboard: (id: string) => void;
  onCreateDashboard: () => void;
  createDashboardDisabled: boolean;
  createDashboardDisabledReason?: string | undefined;
  onOpenResource: (path: string) => void;
}) {
  const { t, i18n } = useTranslation('onboarding');
  const resources = [
    { label: t('home.resources.dashboards'), value: counts.dashboards, to: '/dashboards' },
    { label: t('home.resources.alerts'), value: counts.rules, to: '/alerts' },
    { label: t('home.resources.pipelines'), value: counts.pipelines, to: '/pipelines' },
    { label: t('home.resources.functions'), value: counts.functions, to: '/functions' },
  ];
  return (
    <Card className="overflow-hidden">
      <CardHeader
        title={t('home.dashboards.title')}
        actions={
          <CardHeaderAction
            label={t('home.view_all')}
            onClick={() => onOpenResource('/dashboards')}
          />
        }
      />
      {loading ? (
        <div className="grid min-h-[96px] place-items-center font-sans text-xs text-tx-2">
          {t('home.loading')}
        </div>
      ) : dashboards.length === 0 ? (
        <div className="flex min-h-[96px] flex-wrap items-center justify-between gap-4 px-5 py-4">
          <div>
            <div className="font-sans text-sm font-strong text-tx-1">
              {t('home.dashboards.empty_title')}
            </div>
            <div className="mt-1 font-sans text-xs text-tx-2">
              {t('home.dashboards.empty_description')}
            </div>
          </div>
          <ChromeButton
            variant="primary"
            size="sm"
            disabled={createDashboardDisabled}
            disabledReason={createDashboardDisabledReason}
            onClick={onCreateDashboard}
          >
            <Plus className="h-3 w-3" />
            {t('home.toolbar.new_dashboard')}
          </ChromeButton>
        </div>
      ) : (
        <div
          className={cn(
            'grid gap-px bg-bd-0 sm:grid-cols-2',
            dashboards.length >= 4
              ? 'xl:grid-cols-4'
              : dashboards.length === 3
                ? 'xl:grid-cols-3'
                : dashboards.length === 2
                  ? 'xl:grid-cols-2'
                  : 'xl:grid-cols-1',
          )}
        >
          {dashboards.map((dashboard) => (
            <button
              key={dashboard.id}
              type="button"
              onClick={() => onOpenDashboard(dashboard.id)}
              className="min-w-0 bg-bg-1 px-5 py-4 text-left hover:bg-bg-2"
            >
              <div className="flex items-center gap-2">
                <LayoutDashboard className="h-4 w-4 shrink-0 text-purple-soft" />
                <span className="truncate font-sans text-sm font-strong text-tx-0">
                  {dashboard.title}
                </span>
              </div>
              <div className="mt-2 font-sans text-xs text-tx-2">
                {t('home.dashboards.panel_count', { count: dashboardPanelCount(dashboard) })}
                {' · '}
                {formatRelativeMicros(
                  dashboard.updated_at || dashboard.created_at,
                  i18n.resolvedLanguage ?? i18n.language,
                )}
              </div>
            </button>
          ))}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-x-1 border-t border-bd-0 bg-bg-2/50 px-3 py-2">
        <span className="px-2 font-sans text-xs text-tx-3">{t('home.resources.title')}</span>
        {resources.map((resource) => (
          <button
            key={resource.to}
            type="button"
            onClick={() => onOpenResource(resource.to)}
            className="rounded px-2 py-1 font-sans text-xs text-tx-2 hover:bg-bg-3 hover:text-tx-0"
          >
            <span className="font-strong text-tx-1">{resource.value}</span> {resource.label}
          </button>
        ))}
      </div>
    </Card>
  );
}
