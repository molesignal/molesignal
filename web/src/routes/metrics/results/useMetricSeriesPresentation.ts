import * as React from 'react';

import {
  analyzeMetricSeries,
  rowsToSeries,
} from '@/lib/metricsSeries';
import type { QueryResult } from '@/types/query';
import { timeSeriesColors, timeSeriesKey } from '@/viz/timeseries/colors';

import { timestampsForSeries } from '../model';
import { metricsLegendNames } from '../queryOptions/model';

interface MetricSeriesPresentationInput {
  result: QueryResult | undefined;
  metricName: string | null;
  legend: string | undefined;
  unit: string | undefined;
  xDomain: [number, number];
}

export function useMetricSeriesPresentation({
  result,
  metricName,
  legend,
  unit,
  xDomain,
}: MetricSeriesPresentationInput) {
  const metricSeries = React.useMemo(
    () => (result ? rowsToSeries(result) : []),
    [result],
  );
  const colors = React.useMemo(
    () =>
      timeSeriesColors(
        metricSeries.map((item) =>
          timeSeriesKey({ name: item.valueColumn, labels: item.labels }),
        ),
      ),
    [metricSeries],
  );
  const displayNames = React.useMemo(
    () => metricsLegendNames(metricSeries, metricName ?? undefined, legend),
    [legend, metricName, metricSeries],
  );
  const chartSeries = React.useMemo(
    () =>
      metricSeries.map((series, index) => ({
        id: timeSeriesKey({
          name: series.valueColumn,
          labels: series.labels,
        }),
        name: displayNames[index]!,
        color: colors[index]!,
        data: series.values.map((value) =>
          Number.isFinite(value) ? value : null,
        ),
        timestamps: timestampsForSeries(
          series.timestamps,
          series.values.length,
          xDomain,
        ),
        labels: series.labels,
        ...(unit ? { unit } : {}),
      })),
    [colors, displayNames, metricSeries, unit, xDomain],
  );
  const quality = React.useMemo(
    () => analyzeMetricSeries(metricSeries),
    [metricSeries],
  );

  return {
    metricSeries,
    chartSeries,
    quality,
  };
}
