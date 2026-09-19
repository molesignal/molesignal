import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';

import { metricChartTitle } from './model';

const t = ((key: string, values?: { metric?: string }) => {
  if (key === 'explore.chart.generic_rate') {
    return `${values?.metric ?? ''} rate`;
  }
  if (key === 'explore.chart.query_result') return 'Metric query result';
  return key;
}) as TFunction<'metrics'>;

describe('metricChartTitle', () => {
  it('preserves the metric identifier instead of turning it into prose', () => {
    expect(metricChartTitle('cache_misses_total', 'cache_misses_total', t))
      .toBe('cache_misses_total');
    expect(
      metricChartTitle(
        'cache_misses_total',
        'rate(cache_misses_total[5m])',
        t,
      ),
    ).toBe('cache_misses_total rate');
  });
});
