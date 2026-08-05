import { describe, expect, it } from 'vitest';

import {
  DEFAULT_METRICS_QUERY_OPTIONS,
  isValidMetricsStep,
  metricsLegendNames,
  metricsQueryLimit,
  parseMetricsStepMilliseconds,
} from './model';

describe('metrics query options', () => {
  it('parses Prometheus step durations and rejects incomplete values', () => {
    expect(parseMetricsStepMilliseconds('30s')).toBe(30_000);
    expect(parseMetricsStepMilliseconds('1h30m')).toBe(5_400_000);
    expect(parseMetricsStepMilliseconds('auto')).toBeNull();
    expect(isValidMetricsStep('5 minutes')).toBe(false);
  });

  it('maps range step and instant type to the existing query limit contract', () => {
    const oneHourInMicros = 3_600_000_000;
    expect(
      metricsQueryLimit(
        { ...DEFAULT_METRICS_QUERY_OPTIONS, step: '1m' },
        0,
        oneHourInMicros,
      ),
    ).toBe(60);
    expect(
      metricsQueryLimit(
        { ...DEFAULT_METRICS_QUERY_OPTIONS, type: 'instant' },
        0,
        oneHourInMicros,
      ),
    ).toBe(1);
  });

  it('renders Grafana legend templates with labels and metric name', () => {
    expect(
      metricsLegendNames(
        [{
          labels: { service: 'checkout', status: '500' },
          values: [1],
          timestamps: [1],
          valueColumn: 'value',
        }],
        'http_requests_total',
        '{{service}} · {{status}}',
      )[0],
    ).toBe('checkout · 500');
  });

  it('keeps the full metric identity in Auto mode', () => {
    expect(
      metricsLegendNames(
        [
          {
            labels: { service: 'checkout', status: '500' },
            values: [1],
            timestamps: [1],
            valueColumn: 'value',
          },
          {
            labels: { service: 'checkout', status: '503' },
            values: [2],
            timestamps: [1],
            valueColumn: 'value',
          },
        ],
        'http_requests_total',
        '__auto',
      ),
    ).toEqual([
      'http_requests_total{service="checkout", status="500"}',
      'http_requests_total{service="checkout", status="503"}',
    ]);
  });
});
