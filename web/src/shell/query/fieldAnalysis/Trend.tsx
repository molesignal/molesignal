import * as React from 'react';

import { useTimeFormatter } from '@/lib/time';
import { TIME_SERIES_COLORS } from '@/viz/timeseries/colors';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import type { FieldValueTrend } from './model';

/** The colour the nth of the values that are followed is drawn in, on the chart and in its legend. */
export function trendColor(index: number): string {
  return TIME_SERIES_COLORS[index % TIME_SERIES_COLORS.length]!;
}

interface FieldValueTrendChartProps {
  trend: FieldValueTrend;
  /** How a value is named, where it has a way of its own. */
  label: (value: string) => string;
  /** Names the chart for assistive technology. */
  ariaLabel: string;
}

/**
 * The commonest values of a field over time, a line for each: how many rows
 * hold the value in each stretch. It has no legend of its own: the list of the
 * commonest values under it names the lines, each by its colour.
 */
export function FieldValueTrendChart({ trend, label, ariaLabel }: FieldValueTrendChartProps) {
  const { tz } = useTimeFormatter();
  const series = React.useMemo(() => {
    const stretches = Math.max(0, ...trend.series.map((item) => item.counts.length));
    // A point stands at the start of the stretch it counts.
    const timestamps = Array.from({ length: stretches }, (_, index) => trend.originMicros + index * trend.bucketMicros);

    return trend.series.map((item, index) => ({
      id: item.value,
      name: label(item.value),
      data: item.counts,
      timestamps,
      color: trendColor(index),
    }));
  }, [label, trend]);
  const stretches = series[0]?.data.length ?? 0;

  return (
    <div data-field-analysis-trend>
      <TimeSeriesChart
        series={series}
        height={150}
        xDomain={[trend.originMicros, trend.originMicros + stretches * trend.bucketMicros]}
        timezone={tz}
        showLegend={false}
        // Not tied to the other charts of the page: it has a time range of its own.
        cursorScopeId={null}
        ariaLabel={ariaLabel}
        options={{
          drawStyle: 'line',
          lineWidth: 1.5,
          showPoints: 'never',
          tooltipMode: 'all',
          tooltipSort: 'desc',
          compactAxes: true,
          leftAxis: { min: 0 },
        }}
      />
    </div>
  );
}
