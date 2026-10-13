import { renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { QUERY_LEGEND_AUTO } from '@/dashboard-engine/query/legend';
import type { QueryResult } from '@/types/query';

import { useMetricSeriesPresentation } from './useMetricSeriesPresentation';

const RESULT: QueryResult = {
  columns: ['_timestamp', 'value', 'status'],
  rows: [
    [1_000_000, 4, '500'],
    [2_000_000, 6, '500'],
    [1_000_000, 1, '502'],
    [2_000_000, 2, '502'],
  ],
  scanned_rows: 4,
  took_ms: 1,
};

function present(legend: string | undefined) {
  return renderHook(() =>
    useMetricSeriesPresentation({
      result: RESULT,
      metricName: 'http_requests_total',
      legend,
      unit: undefined,
      xDomain: [1_000_000, 2_000_000],
    }),
  ).result.current.chartSeries;
}

describe('useMetricSeriesPresentation', () => {
  it('names each series by the legend the user wrote', () => {
    const series = present('{{status}}');

    expect(series.map((item) => item.legendName)).toEqual(['500', '502']);
    expect(series.map((item) => item.name)).toEqual(['500', '502']);
    // The metric stays the metric: label pivots are built from it.
    expect(series.map((item) => item.metricName)).toEqual([
      'http_requests_total',
      'http_requests_total',
    ]);
  });

  it('leaves the name to the metric when the legend only spells out labels', () => {
    for (const legend of [QUERY_LEGEND_AUTO, undefined]) {
      const series = present(legend);

      expect(series).toHaveLength(2);
      expect(series.every((item) => !('legendName' in item))).toBe(true);
      expect(series.map((item) => item.metricName)).toEqual([
        'http_requests_total',
        'http_requests_total',
      ]);
    }
  });
});
