import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as alertsApi from '@/api/alerts';
import { apmApi } from '@/api/apm';
import * as homeApi from '@/api/home';
import * as incidentsApi from '@/api/incidents';
import * as syntheticsApi from '@/api/synthetics';
import { canAccessProductPath, hasPermission, useProductAccess } from '@/product/access';
import type { PermissionKey } from '@/product/permissions';
import { rowsOf } from '@/shell/notifications/model';

import {
  type HomeWindow,
  summarizeChecks,
  summarizeIncidents,
  summarizeRules,
  WINDOW_BUCKETS,
  WINDOW_SECS,
} from './model';

const REFRESH_MS = 60_000;

export const HOME_INCIDENTS_QUERY_KEY = ['home', 'incidents', 'active'] as const;

/** The window ends on the minute, so every panel read in the same minute asks about the same range. */
function windowRange(period: HomeWindow, nowMs: number): { from: number; to: number } {
  const to = Math.floor(nowMs / 60_000) * 60_000 * 1_000;

  return { from: to - WINDOW_SECS[period] * 1_000_000, to };
}

/**
 * Everything Home shows, each part read only by whoever can open the page it comes from: the
 * incidents going on, the rules, the services, the checks and the signals arriving. A part the
 * reader may not see is `allowed: false` and never asked for.
 */
export function useHomeStatus(period: HomeWindow) {
  const access = useProductAccess();
  const [nowMs, setNowMs] = React.useState(() => Date.now());

  React.useEffect(() => {
    const timer = setInterval(() => setNowMs(Date.now()), REFRESH_MS);
    return () => clearInterval(timer);
  }, []);

  const range = React.useMemo(() => windowRange(period, nowMs), [period, nowMs]);
  // A part needs the page it opens and the permission of the answer it reads.
  const can = (path: string, ...permissions: PermissionKey[]) =>
    canAccessProductPath(path, access) && permissions.some((permission) => hasPermission(permission, access));
  const allowed = {
    incidents: can('/alerts/incidents', 'alerts.incidents.read'),
    rules: can('/alerts/rules', 'alerts.rules.read'),
    services: can('/apm/overview', 'apm.services.read', 'sys.telemetry.read'),
    checks: can('/synthetics/checks', 'synthetics.monitors.read'),
    availability: can('/synthetics/checks', 'synthetics.results.read'),
    intake: can('/streams', 'streams.query', 'sys.telemetry.read'),
  };

  const incidents = useQuery({
    queryKey: HOME_INCIDENTS_QUERY_KEY,
    queryFn: () => incidentsApi.list({ scope: 'active' }),
    enabled: allowed.incidents,
    refetchInterval: REFRESH_MS,
  });
  const rules = useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: () => alertsApi.list(),
    enabled: allowed.rules,
    refetchInterval: REFRESH_MS,
  });
  const apm = useQuery({
    queryKey: ['home', 'apm', range],
    queryFn: () => apmApi.overview(range),
    enabled: allowed.services,
    placeholderData: (previous) => previous,
  });
  const monitors = useQuery({
    queryKey: ['synthetics', 'monitors'],
    queryFn: syntheticsApi.listMonitors,
    enabled: allowed.checks,
    refetchInterval: REFRESH_MS,
  });
  const availability = useQuery({
    queryKey: ['home', 'synthetics', 'availability', range],
    queryFn: () => syntheticsApi.getAvailability(range),
    enabled: allowed.checks && allowed.availability,
    placeholderData: (previous) => previous,
  });
  const intake = useQuery({
    queryKey: ['home', 'intake', period, nowMs],
    queryFn: () => homeApi.overview({ windowSecs: WINDOW_SECS[period], bucketCount: WINDOW_BUCKETS[period] }),
    enabled: allowed.intake,
    placeholderData: (previous) => previous,
  });

  const summaries = React.useMemo(
    () => ({
      incidents: incidents.data ? summarizeIncidents(rowsOf(incidents.data)) : undefined,
      rules: rules.data ? summarizeRules(Array.isArray(rules.data) ? rules.data : []) : undefined,
      checks: monitors.data ? summarizeChecks(Array.isArray(monitors.data) ? monitors.data : []) : undefined,
    }),
    [incidents.data, rules.data, monitors.data],
  );

  const fetching = [incidents, rules, apm, monitors, availability, intake].some((query) => query.isFetching);
  const refresh = React.useCallback(() => setNowMs(Date.now()), []);

  return {
    allowed,
    range,
    incidents,
    rules,
    apm,
    monitors,
    availability,
    intake,
    summaries,
    fetching,
    /** Reads every part again over a window ending now. */
    refresh: () => {
      refresh();
      // `refetch` reads even a query that is not enabled: ask only for what the reader may see.
      if (allowed.incidents) void incidents.refetch();
      if (allowed.rules) void rules.refetch();
      if (allowed.checks) void monitors.refetch();
    },
  };
}

export type HomeStatus = ReturnType<typeof useHomeStatus>;
