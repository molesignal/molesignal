import { keepPreviousData, useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useSearchParams } from 'react-router-dom';

import * as rumApi from '@/api/rum';
import type { RumAggregateParams, RumApplication } from '@/api/rum';
import { useAuthStore } from '@/stores/auth';
import { resolveWindow, type TimeWindow } from '@/stores/useTimeStore';

import { type OverviewRange, scopeFilters } from './model';

/** How often a range that ends now moves its end: a page left open keeps up with the minute. */
const REFRESH_MS = 60_000;

/**
 * What the overview is of, in its address: the application (`app`) and the environment (`env`).
 * An address that names no application, or one that no longer exists, is of the first by name; a
 * change of application leaves the environment, which belongs to the one it was chosen in.
 */
export function useOverviewScope(applications: readonly RumApplication[]) {
  const [params, setParams] = useSearchParams();
  const wanted = params.get('app') ?? '';
  const application = applications.find((candidate) => candidate.id === wanted) ?? applications[0] ?? null;
  const environment = params.get('env') ?? '';

  const setApplication = React.useCallback(
    (id: string) =>
      setParams(
        (current) => {
          const next = new URLSearchParams(current);
          next.set('app', id);
          next.delete('env');
          return next;
        },
        { replace: true },
      ),
    [setParams],
  );

  const setEnvironment = React.useCallback(
    (value: string) =>
      setParams(
        (current) => {
          const next = new URLSearchParams(current);
          if (value) next.set('env', value);
          else next.delete('env');
          return next;
        },
        { replace: true },
      ),
    [setParams],
  );

  return { params, application, environment, setApplication, setEnvironment };
}

/** The range a time window stands for, in microseconds; one that ends now moves along once a minute. */
export function useOverviewRange(window: TimeWindow): OverviewRange {
  const relative = window.mode === 'relative';
  const [now, setNow] = React.useState(() => Date.now());

  React.useEffect(() => {
    if (!relative) return undefined;

    const timer = globalThis.setInterval(() => setNow(Date.now()), REFRESH_MS);

    return () => globalThis.clearInterval(timer);
  }, [relative]);

  return React.useMemo(() => {
    const { from, to } = resolveWindow(window, new Date(now));

    return { from: from.getTime() * 1_000, to: to.getTime() * 1_000 };
  }, [now, window]);
}

/**
 * One read of the overview. It keeps the numbers it had while the next are on their way, so a range
 * that moves on does not empty the card; a read that is not asked for stays idle.
 */
export function useAggregate(name: string, params: RumAggregateParams | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'overview', orgId, name, params],
    queryFn: ({ signal }) => rumApi.aggregate(params as RumAggregateParams, signal),
    enabled: Boolean(orgId && params),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}

/** The environments the application has sessions in, over the range. */
export function useEnvironments(application: string, range: OverviewRange) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'overview', orgId, 'environments', application, range],
    queryFn: ({ signal }) =>
      rumApi.facets(
        {
          type: 'sessions',
          from: range.from,
          to: range.to,
          filters: scopeFilters({ application, environment: '' }),
          field: 'env',
          limit: 50,
        },
        signal,
      ),
    enabled: Boolean(orgId && application),
    placeholderData: keepPreviousData,
    staleTime: 60_000,
  });
}
