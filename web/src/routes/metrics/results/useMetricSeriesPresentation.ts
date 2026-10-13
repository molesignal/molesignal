import * as React from 'react';

import { resolveQueryLegendMode } from '@/dashboard-engine/query/legend';
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
  // A legend the user wrote is the name of the series. Auto and Verbose only
  // spell out the label set, which the series table keeps one click away.
  const customLegend = resolveQueryLegendMode(legend) === 'custom';
  const chartSeries = React.useMemo(
    () =>
      metricSeries.map((series, index) => ({
        id: timeSeriesKey({
          name: series.valueColumn,
          labels: series.labels,
        }),
        metricName: metricName?.trim() || series.valueColumn,
        ...(customLegend ? { legendName: displayNames[index]! } : {}),
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
    [colors, customLegend, displayNames, metricName, metricSeries, unit, xDomain],
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
