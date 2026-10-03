import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

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
import { toApiError } from '@/lib/http';
import { deriveActivationState } from '@/product/activation';
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
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const currentUserId = useAuthStore((state) => state.ctx?.user_id ?? '');
  const users = useUsers();
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
  });
  const streamsQuery = useQuery({
    queryKey: ['streams', 'list'],
    queryFn: () => streamsApi.list(200),
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
  });
  const incidentsQuery = useQuery({
    queryKey: ['alerts', 'incidents'],
    queryFn: () =>
      incidentsApi.list({ scope: 'all', window_secs: 14 * 24 * 60 * 60 }),
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

  const overview = overviewQuery.data;
  const runtimeOverview = runtimeQuery.data;
  const fleet = React.useMemo<FleetSummary | undefined>(() => {
    if (runtimeQuery.isError) return UNKNOWN_FLEET;
    if (!runtimeOverview) return undefined;
    // Partial or empty bodies happen (proxies, mocks, older backends): an
    // absent field means "nothing reported", never a crash on the home page.
    return summarizeFleet(
      runtimeOverview.streams ?? [],
      runtimeOverview.generated_at_micros != null
        ? toMicros(runtimeOverview.generated_at_micros)
        : Date.now() * 1000,
    );
  }, [runtimeOverview, runtimeQuery.isError]);

  const streams = streamsQuery.data ?? [];
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
    () => selectFeaturedOnCall(schedules, currentUserId, nowMicros),
    [currentUserId, nowMicros, schedules],
  );
  const featuredTeamName = featuredOnCall?.schedule.team_id
    ? teamsById.get(featuredOnCall.schedule.team_id)?.name
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
    streamsQuery,
    dashboardsQuery,
    incidentsQuery,
    rulesQuery,
    pipelinesQuery,
    activityQuery,
    schedulesQuery,
    escalationPoliciesQuery,
    teamsQuery,
  ];
  const isRefreshing = queries.some((query) => query.isFetching);
  const refresh = async () => {
    await Promise.all(queries.map((query) => query.refetch()));
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
    criticalItems,
    activity,
    activityState: queryStateFor({
      isLoading: activityQuery.isLoading,
      isError: activityQuery.isError,
      data: activity,
    }),
    activityError: activityQuery.error,
    activation,
    loadSample,
    featuredOnCall,
    featuredTeamName,
    shiftOverview,
    schedulesLoading: schedulesQuery.isLoading,
    users,
    nowMicros,
    isRefreshing,
    refresh,
    /** Epoch ms of the last successful overview fetch (0 before the first). */
    updatedAt: overviewQuery.dataUpdatedAt,
  };
}
