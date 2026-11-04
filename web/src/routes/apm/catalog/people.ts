import { useQueries, useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as schedulesApi from '@/api/schedules';
import * as teamsApi from '@/api/teams';

/** The organization's teams by id: who owns a service is one of them. */
export function useTeams() {
  const query = useQuery({
    queryKey: ['teams'],
    queryFn: () => teamsApi.list(),
    staleTime: 60_000,
  });
  const byId = React.useMemo(() => new Map((query.data ?? []).map((team) => [team.id, team])), [query.data]);

  return { teams: query.data ?? [], byId, query };
}

/**
 * Who is on call for each team now: the first enabled schedule a team has,
 * asked who holds it. Teams without a schedule have nobody.
 */
export function useOnCall(teamIds: readonly string[]) {
  const schedules = useQuery({
    queryKey: ['schedules'],
    queryFn: () => schedulesApi.list(),
    staleTime: 60_000,
  });

  const scheduleByTeam = React.useMemo(() => {
    const wanted = new Set(teamIds);
    const map = new Map<string, string>();

    for (const schedule of schedules.data ?? []) {
      if (!schedule.enabled || !schedule.teamId || !wanted.has(schedule.teamId) || map.has(schedule.teamId)) continue;
      map.set(schedule.teamId, schedule.id);
    }

    return map;
  }, [schedules.data, teamIds]);

  const entries = [...scheduleByTeam.entries()];
  const answers = useQueries({
    queries: entries.map(([, scheduleId]) => ({
      queryKey: ['schedules', scheduleId, 'on-call'],
      queryFn: () => schedulesApi.whoIsOnCall(scheduleId),
      staleTime: 60_000,
    })),
  });

  const byTeam = new Map<string, string>();

  entries.forEach(([teamId], index) => {
    const userId = answers[index]?.data?.userId;
    if (userId) byTeam.set(teamId, userId);
  });

  return byTeam;
}
