import { describe, expect, it } from 'vitest';

import type { MetricCatalogEntry } from '@/api/metricsCatalog';

import { metricTypeAbbreviation, resolveMetricType } from './metricTypes';

function metric(name: string, metricType?: MetricCatalogEntry['metric_type']): MetricCatalogEntry {
  return {
    name,
    labels: [],
    field_count: 1,
    ...(metricType ? { metric_type: metricType } : {}),
  };
}

describe('resolveMetricType', () => {
  it('prefers the metric type supplied by the catalog', () => {
    expect(resolveMetricType(metric('custom_value', 'counter'))).toBe('counter');
  });

  it('classifies legacy catalog entries using Prometheus naming conventions', () => {
    expect(resolveMetricType(metric('http_requests_total'))).toBe('counter');
    expect(resolveMetricType(metric('http_request_duration_seconds'))).toBe('histogram');
    expect(resolveMetricType(metric('process_resident_memory_bytes'))).toBe('gauge');
  });

  it('uses the compact labels shown by the metrics explorer', () => {
    expect(metricTypeAbbreviation('counter')).toBe('COUN');
    expect(metricTypeAbbreviation('histogram')).toBe('HIST');
    expect(metricTypeAbbreviation('gauge')).toBe('GAUG');
  });
});
