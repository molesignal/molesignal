import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as queryApi from '@/api/query';
import { compactMetricSeriesName, rowsToSeries } from '@/lib/metricsSeries';
import { useAuthStore } from '@/stores/auth';
import { comparePeriods, type CompareBaseline, type Period } from '@/time/baseline/periods';
import { timeSeriesKey } from '@/viz/timeseries/colors';
import type { TimeSeriesSeries } from '@/viz/timeseries/types';

import { timestampsForSeries } from '../model';
import { metricsQueryRange, type MetricsQueryOptions } from '../queryOptions/model';

/** How a baseline series is drawn beside the series it is the baseline of. */
const BASELINE_DASH = [4, 4] as const;

/**
 * A moment moved by `micros`, in the unit it is written in: the chart reads
 * seconds, milliseconds and microseconds by their size, and so does this.
 */
export function shifted(timestamp: number, micros: number): number {
  const per = timestamp > 1e14 ? 1 : timestamp > 1e11 ? 1_000 : 1_000_000;
  return timestamp + micros / per;
}

export interface BaselineOverlay {
  /** The baseline's series, moved onto the period looked at. */
  series: TimeSeriesSeries[];
  /** The stretch the baseline stands for, while one is chosen. */
  resolved: Period | null;
  pending: boolean;
  failed: boolean;
}

/**
 * The same query over the baseline, laid on the chart: each series read over
 * the baseline and moved forward onto the period looked at, in the colour of
 * the series it is the baseline of, dashed. Only a range query has one.
 */
export function useBaselineOverlay({
  statement,
  comparison,
  baseline,
  queryOptions,
  current,
}: {
  statement: string | null;
  comparison: Period;
  baseline: CompareBaseline | null;
  queryOptions: MetricsQueryOptions;
  current: readonly TimeSeriesSeries[];
}): BaselineOverlay {
  const { t } = useTranslation('metrics');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const periods = baseline ? comparePeriods(comparison, baseline) : null;
  const resolved = periods?.baseline ?? null;
  const ranged = queryOptions.type === 'range';

  const answer = useQuery({
    queryKey: ['metrics', 'baseline', orgId, statement, resolved?.from, resolved?.to, queryOptions.step],
    enabled: Boolean(orgId && statement && resolved && ranged),
    queryFn: ({ signal }) =>
      queryApi.readQuery(
        {
          orgId,
          language: 'promql',
          statement: statement!,
          timeRange: { start: resolved!.from, end: resolved!.to },
          ...metricsQueryRange(queryOptions, resolved!.from, resolved!.to),
        },
        signal,
      ),
    retry: false,
    staleTime: 60_000,
  });

  const result = baseline && ranged ? answer.data : undefined;

  const series = React.useMemo(() => {
    if (!result || !resolved) return [];
    const shift = comparison.from - resolved.from;
    const byId = new Map(current.map((item) => [item.id, item]));

    return rowsToSeries(result).map((item) => {
      const id = timeSeriesKey({ name: item.valueColumn, labels: item.labels });
      const own = byId.get(id);

      const name = t('explore.baseline.series', { name: own?.legendName ?? own?.metricName ?? own?.name ?? compactMetricSeriesName(item) });

      return {
        id: `baseline:${id}`,
        // Explore legends lead with the name an author gave a series: the baseline says it is one.
        legendName: name,
        name,
        data: item.values.map((value) => (Number.isFinite(value) ? value : null)),
        timestamps: timestampsForSeries(item.timestamps, item.values.length, [resolved.from, resolved.to]).map(
          (timestamp) => shifted(timestamp, shift),
        ),
        labels: item.labels,
        dash: BASELINE_DASH,
        lineWidth: 1,
        ...(own?.color ? { color: own.color } : {}),
        ...(own?.unit ? { unit: own.unit } : {}),
      };
    });
  }, [comparison.from, current, resolved, result, t]);

  return { series, resolved, pending: answer.isFetching, failed: answer.isError };
}
