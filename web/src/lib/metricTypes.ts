import type { MetricCatalogEntry, MetricType } from '@/api/metricsCatalog';

/**
 * Resolve the catalog-provided Prometheus metric kind while remaining
 * compatible with older servers that did not expose `metric_type`.
 */
export function resolveMetricType(metric: MetricCatalogEntry): MetricType {
  if (metric.metric_type) return metric.metric_type;

  const name = metric.name.toLowerCase();
  if (
    name.endsWith('_bucket') ||
    /(?:^|_)(?:duration|latency|histogram)(?:_|$)/.test(name)
  ) {
    return 'histogram';
  }
  if (
    name.endsWith('_total') ||
    name.endsWith('_count') ||
    name.endsWith('_sum')
  ) {
    return 'counter';
  }
  return 'gauge';
}

export function metricTypeAbbreviation(type: MetricType): 'COUN' | 'HIST' | 'GAUG' {
  if (type === 'counter') return 'COUN';
  if (type === 'histogram') return 'HIST';
  return 'GAUG';
}
