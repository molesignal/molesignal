import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as modelProvidersApi from '@/api/agent/modelProviders';
import * as alertsApi from '@/api/alerts';
import * as auditApi from '@/api/audit';
import * as dashboardsApi from '@/api/dashboards';
import * as escalationsApi from '@/api/escalations';
import * as homeApi from '@/api/home';
import * as incidentsApi from '@/api/incidents';
import * as notifyApi from '@/api/notify';
import * as onboardingApi from '@/api/onboarding';
import * as pipelinesApi from '@/api/pipelines';
import * as schedulesApi from '@/api/schedules';
import * as streamsApi from '@/api/streams';
import * as teamsApi from '@/api/teams';
import { toApiError } from '@/lib/http';
import { useProductAccess } from '@/product/access';
import {
  canReadActivationStep,
  deriveActivationState,
} from '@/product/activation';
import type { CriticalAlertItem } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import { useAuthStore } from '@/stores/auth';

import { formatAge } from './canvas/format';
import { HOME_RECENT_ACTIVITY_LIMIT } from './canvas/RecentActivitySection';
import {
  type FleetSummary,
  summarizeFleet,
  toMicros,
  UNKNOWN_FLEET,
} from './canvas/streamCondition';
import { selectFeaturedOnCall, summarizeOnCallShift } from './onCall/model';

/**
 * Everything Home reads from the backend, plus the values derived from it.
 * Rendering stays in the route; this owns the queries so the route reads as
 * composition.
 */
export function useHomeData(windowSecs: number) {
  const { t } = useTranslation('onboarding');
  const qc = useQueryClient();
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const currentUserId = useAuthStore((state) => state.ctx?.userId ?? '');
  const users = useUsers();
  const access = useProductAccess();
  const canReadSchedules = canReadActivationStep('schedule', access);
  const [nowMicros, setNowMicros] = React.useState(() => Date.now() * 1000);

  React.useEffect(() => {
    const timer = window.setInterval(
      () => setNowMicros(Date.now() * 1000),
      60_000,
    );
    return () => window.clearInterval(timer);
  }, []);

  const bucketCount = windowSecs > 24 * 60 * 60 ? 28 : 24;
  const overviewQuery = useQuery({
    queryKey: ['home', 'overview', orgId, windowSecs],
    queryFn: () => homeApi.overview({ windowSecs, bucketCount }),
    enabled: Boolean(orgId),
    refetchInterval: 60_000,
  });
  const sampleStatusQuery = useQuery({
    queryKey: ['onboarding', 'sample-data'],
    queryFn: onboardingApi.getSampleDataStatus,
    enabled: canReadActivationStep('sample-data', access),
  });
  const streamsQuery = useQuery({
    queryKey: ['streams', 'list'],
    queryFn: () => streamsApi.list(200),
    enabled: canReadActivationStep('datasource', access),
  });
  const runtimeQuery = useQuery({
    queryKey: ['streams', 'runtime', windowSecs],
    queryFn: () => streamsApi.runtimeOverview({ windowSecs, bucketCount }),
    enabled: Boolean(orgId),
    refetchInterval: 60_000,
  });
  const dashboardsQuery = useQuery({
    queryKey: ['dashboards', 'list'],
    queryFn: () => dashboardsApi.list(),
    enabled: canReadActivationStep('dashboard', access),
  });
  const incidentsQuery = useQuery({
    queryKey: ['alerts', 'incidents'],
    queryFn: () =>
      incidentsApi.list({ scope: 'all', windowSecs: 14 * 24 * 60 * 60 }),
  });
  const rulesQuery = useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: () => alertsApi.list(),
    enabled: canReadActivationStep('alert', access),
  });
  const pipelinesQuery = useQuery({
    queryKey: ['pipelines', 'list'],
    queryFn: () => pipelinesApi.list(),
    enabled: canReadActivationStep('pipeline', access),
  });
  const notifyConnectorsQuery = useQuery({
    queryKey: ['notify', 'connectors'],
    queryFn: notifyApi.listConnectors,
    enabled: canReadActivationStep('notification', access),
  });
  const modelProvidersQuery = useQuery({
    queryKey: ['agent', 'model-providers'],
    queryFn: modelProvidersApi.list,
    enabled: canReadActivationStep('ai', access),
  });
  const activityQuery = useQuery({
    queryKey: ['audit', 'recent', HOME_RECENT_ACTIVITY_LIMIT],
    queryFn: () => auditApi.recent(HOME_RECENT_ACTIVITY_LIMIT),
  });
  const schedulesQuery = useQuery({
    queryKey: ['schedules'],
    queryFn: schedulesApi.list,
    refetchInterval: 60_000,
    enabled: canReadSchedules,
  });
  // These two only fill in the on-call card, which shows nothing without the schedules.
  const escalationPoliciesQuery = useQuery({
    queryKey: ['escalation-policies'],
    queryFn: escalationsApi.list,
    enabled: canReadSchedules,
  });
  const teamsQuery = useQuery({
    queryKey: ['teams'],
    queryFn: teamsApi.list,
    enabled: canReadSchedules,
  });

  const loadSample = useMutation({
    mutationFn: onboardingApi.loadSampleData,
    onSuccess: (result) => {
      toast.success(t('activation.load_success', { rows: result.totalRows }));
      void qc.invalidateQueries({ queryKey: ['onboarding', 'sample-data'] });
      void qc.invalidateQueries({ queryKey: ['streams'] });
      void qc.invalidateQueries({ queryKey: ['home', 'overview'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const overview = overviewQuery.data;
  const runtimeOverview = runtimeQuery.data;
  const fleet = React.useMemo<FleetSummary | undefined>(() => {
    if (runtimeQuery.isError) return UNKNOWN_FLEET;
    if (!runtimeOverview) return undefined;
    // Partial or empty bodies happen (proxies, mocks, older backends): an
    // absent field means "nothing reported", never a crash on the home page.
    return summarizeFleet(
      runtimeOverview.streams ?? [],
      runtimeOverview.generatedAt != null
        ? toMicros(runtimeOverview.generatedAt)
        : Date.now() * 1000,
    );
  }, [runtimeOverview, runtimeQuery.isError]);

  const dashboards = dashboardsQuery.data ?? [];
  const rules = rulesQuery.data ?? [];
  const pipelines = pipelinesQuery.data ?? [];
  const activity = activityQuery.data ?? [];
  const incidents = React.useMemo(
    () => incidentsQuery.data ?? [],
    [incidentsQuery.data],
  );
  const escalationPolicies = React.useMemo(
    () => escalationPoliciesQuery.data ?? [],
    [escalationPoliciesQuery.data],
  );
  const schedules = React.useMemo(
    () => schedulesQuery.data ?? [],
    [schedulesQuery.data],
  );
  const teams = React.useMemo(() => teamsQuery.data ?? [], [teamsQuery.data]);
  const teamsById = React.useMemo(
    () => new Map(teams.map((team) => [team.id, team])),
    [teams],
  );

  const firing = incidents.filter((incident) => incident.status === 'open').length;
  const acknowledged = incidents.filter(
    (incident) => incident.status === 'acknowledged',
  ).length;
  const criticalItems: CriticalAlertItem[] = incidents
    .filter((incident) => incident.status === 'open')
    .slice(0, 5)
    .map((incident) => {
      const service = incident.affectedServices?.[0];
      const age = formatAge(incident.createdAt);
      return {
        id: incident.id,
        label: incident.summary || incident.id,
        meta: service ? `${service} · ${age}` : age,
      };
    });

  const activation = deriveActivationState(
    {
      streams: streamsQuery.data,
      dashboardsCount: dashboards.length,
      alertsCount: rules.length,
      schedulesCount: schedules.length,
      notifyConnectors: notifyConnectorsQuery.data ?? [],
      modelProviders: modelProvidersQuery.data ?? [],
      pipelinesCount: pipelines.length,
      sampleDataAvailable: sampleStatusQuery.data?.loaded ?? false,
    },
    access,
  );

  const featuredOnCall = React.useMemo(
    () => selectFeaturedOnCall(schedules, currentUserId, nowMicros),
    [currentUserId, nowMicros, schedules],
  );
  const featuredTeamName = featuredOnCall?.schedule.teamId
    ? teamsById.get(featuredOnCall.schedule.teamId)?.name
    : undefined;
  const shiftOverview = React.useMemo(
    () =>
      featuredOnCall &&
      !incidentsQuery.isLoading &&
      !incidentsQuery.isError &&
      !escalationPoliciesQuery.isLoading &&
      !escalationPoliciesQuery.isError
        ? summarizeOnCallShift(featuredOnCall, incidents, escalationPolicies)
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

  const queries = [
    overviewQuery,
    runtimeQuery,
    sampleStatusQuery,
    streamsQuery,
    dashboardsQuery,
    incidentsQuery,
    rulesQuery,
    pipelinesQuery,
    notifyConnectorsQuery,
    modelProvidersQuery,
    activityQuery,
    schedulesQuery,
    escalationPoliciesQuery,
    teamsQuery,
  ];
  const isRefreshing = queries.some((query) => query.isFetching);
  const refresh = async () => {
    // `refetch` runs a query whatever its `enabled` says, and the backend would refuse these.
    await Promise.all(
      queries.filter((query) => query.isEnabled).map((query) => query.refetch()),
    );
  };

  return {
    overview,
    overviewState: queryStateFor({
      isLoading: overviewQuery.isLoading,
      isError: overviewQuery.isError,
      data: overview?.streams,
    }),
    overviewLoading: overviewQuery.isLoading,
    overviewError: overviewQuery.error,
    runtimeOverview,
    runtimeLoading: runtimeQuery.isLoading,
    fleet,
    dashboards,
    dashboardsLoading: dashboardsQuery.isLoading,
    firing,
    acknowledged,
    incidentsLoading: incidentsQuery.isLoading,
    incidentsFailed: incidentsQuery.isError,
    criticalItems,
    activity,
    activityState: queryStateFor({
      isLoading: activityQuery.isLoading,
      isError: activityQuery.isError,
      data: activity,
    }),
    activityError: activityQuery.error,
    activation,
    /** The steps left can only be counted once every list the reader may see has answered. */
    activationKnown:
      access?.status === 'ready' &&
      [
        streamsQuery,
        dashboardsQuery,
        rulesQuery,
        schedulesQuery,
        notifyConnectorsQuery,
        modelProvidersQuery,
        pipelinesQuery,
        sampleStatusQuery,
      ].every((query) => !query.isEnabled || query.isSuccess),
    loadSample,
    canReadSchedules,
    featuredOnCall,
    featuredTeamName,
    shiftOverview,
    schedulesLoading: schedulesQuery.isLoading,
    schedulesError: schedulesQuery.error,
    users,
    nowMicros,
    isRefreshing,
    refresh,
    /** Epoch ms of the last successful overview fetch (0 before the first). */
    updatedAt: overviewQuery.dataUpdatedAt,
  };
}
