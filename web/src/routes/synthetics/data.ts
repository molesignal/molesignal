import { useQueries, useQuery } from '@tanstack/react-query';

import * as syntheticsApi from '@/api/synthetics';
import type {
  MonitorDetail,
  MonitorKind,
  MonitorRevision,
  SyntheticMonitor,
  SyntheticResult,
} from '@/api/synthetics';
import type { TimeWindow } from '@/stores/useTimeStore';

import {
  activeRevision,
  operationalResults,
  publishedRevision,
  resultDurationMicros,
  successRate,
} from './model';
import { windowMicros } from './time';

export interface CheckRow {
  monitor: SyntheticMonitor;
  detail: MonitorDetail | undefined;
  revision: MonitorRevision | undefined;
  results: SyntheticResult[];
  latestResult: SyntheticResult | undefined;
  latestLatencyMicros: number | undefined;
  successRate: number | undefined;
}

/** How often a tab asks again: checks run on their own, so the page follows without being told to. */
export const SYNTHETICS_REFRESH_MILLIS = 30_000;

/**
 * How the checks, or one check, did over a range. The range is asked for as it is written, so a
 * relative one follows the clock each time the query runs again.
 */
export function useSyntheticsAvailability(window: TimeWindow, monitorId?: string) {
  return useQuery({
    queryKey: ['synthetics', 'availability', window.from, window.to, window.mode, monitorId ?? null],
    queryFn: () =>
      syntheticsApi.getAvailability({
        ...windowMicros(window),
        ...(monitorId ? { monitorId: monitorId } : {}),
      }),
    staleTime: 10_000,
    refetchInterval: SYNTHETICS_REFRESH_MILLIS,
  });
}

export function useSyntheticsWorkspace(options: {
  kinds?: MonitorKind[] | undefined;
  resultLimit?: number | undefined;
  includeResults?: boolean | undefined;
  includeArchived?: boolean | undefined;
} = {}) {
  const monitorQuery = useQuery({
    queryKey: ['synthetics', 'monitors'],
    queryFn: syntheticsApi.listMonitors,
    refetchInterval: SYNTHETICS_REFRESH_MILLIS,
  });
  const locationQuery = useQuery({
    queryKey: ['synthetics', 'locations'],
    queryFn: syntheticsApi.listLocations,
    refetchInterval: SYNTHETICS_REFRESH_MILLIS,
  });
  const monitors = (monitorQuery.data ?? []).filter(
    (monitor) =>
      (options.includeArchived || monitor.lifecycle !== 'archived') &&
      (!options.kinds || options.kinds.includes(monitor.kind)),
  );
  const details = useQueries({
    queries: monitors.map((monitor) => ({
      queryKey: ['synthetics', 'monitor', monitor.id],
      queryFn: () => syntheticsApi.getMonitor(monitor.id),
      staleTime: 15_000,
      // Where a check is missing runs comes with it, and changes as runs are missed.
      refetchInterval: SYNTHETICS_REFRESH_MILLIS,
    })),
  });
  const results = useQueries({
    queries:
      options.includeResults === false
        ? []
        : monitors.map((monitor) => ({
            queryKey: ['synthetics', 'monitor', monitor.id, 'results', options.resultLimit ?? 20],
            queryFn: () =>
              syntheticsApi.listResults(monitor.id, { limit: options.resultLimit ?? 20 }),
            staleTime: 10_000,
            refetchInterval: SYNTHETICS_REFRESH_MILLIS,
          })),
  });
  const rows: CheckRow[] = monitors.map((monitor, index) => {
    const detail = details[index]?.data;
    const monitorResults = results[index]?.data ?? [];
    const metricResults = operationalResults(monitorResults);
    // A skipped run did not happen: the last run and its time are those of the latest that did.
    const latestResult = metricResults.find((result) => result.outcome !== 'skipped');
    return {
      monitor,
      detail,
      revision: publishedRevision(detail) ?? activeRevision(detail),
      results: monitorResults,
      latestResult,
      latestLatencyMicros: resultDurationMicros(latestResult),
      successRate: successRate(metricResults),
    };
  });
  const allResults = rows
    .flatMap((row) => row.results)
    .sort((left, right) => right.startedAt - left.startedAt);
  const allOperationalResults = operationalResults(allResults);
  const error =
    monitorQuery.error ??
    locationQuery.error ??
    details.find((query) => query.error)?.error ??
    results.find((query) => query.error)?.error;
  return {
    monitorQuery,
    locationQuery,
    rows,
    allResults,
    operationalResults: allOperationalResults,
    locations: locationQuery.data ?? [],
    pending:
      monitorQuery.isPending ||
      locationQuery.isPending ||
      details.some((query) => query.isPending) ||
      results.some((query) => query.isPending),
    refetching:
      monitorQuery.isRefetching ||
      locationQuery.isRefetching ||
      details.some((query) => query.isRefetching) ||
      results.some((query) => query.isRefetching),
    error,
    refetch: async () => {
      await Promise.all([
        monitorQuery.refetch(),
        locationQuery.refetch(),
        ...details.map((query) => query.refetch()),
        ...results.map((query) => query.refetch()),
      ]);
    },
  };
}
