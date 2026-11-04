import type { ErrorSummary, RedSummary, TrendPoint } from '@/api/apm';
import type { AlertRule, ApmAlertMetric, ApmAlertQuery } from '@/types/alerting';
import type { QueryResult } from '@/types/query';

/** The figures an APM rule can watch, the usual ones first. The burn rate of an SLO is set from its page. */
export const APM_METRICS: readonly ApmAlertMetric[] = [
  'error_rate',
  'p95',
  'p99',
  'p90',
  'p50',
  'average',
  'throughput',
  'error_count',
  'new_error_groups',
  'request_count',
  'apdex',
];

/** The figures an anomaly rule watches against the days before: new error groups are watched against thresholds. */
export const APM_ANOMALY_METRICS: readonly ApmAlertMetric[] = APM_METRICS.filter((metric) => metric !== 'new_error_groups');

export const APM_TRANSACTION_KINDS = ['http', 'rpc', 'messaging', 'span', 'other'] as const;

/** The unit a figure's thresholds are set in. */
export function apmMetricUnit(metric: ApmAlertMetric): 'percent' | 'milliseconds' | 'per_second' | 'count' | 'score' | 'burn_rate' {
  if (metric === 'error_rate') return 'percent';
  if (metric === 'throughput') return 'per_second';
  if (metric === 'request_count' || metric === 'error_count' || metric === 'new_error_groups') return 'count';
  if (metric === 'apdex') return 'score';
  if (metric === 'slo_burn_rate') return 'burn_rate';
  return 'milliseconds';
}

/** A threshold to start from for a figure: a line people commonly page on. */
export function apmDefaultThreshold(metric: ApmAlertMetric): { operator: 'gt' | 'lt'; threshold: number } {
  switch (metric) {
    case 'error_rate':
      return { operator: 'gt', threshold: 5 };
    case 'apdex':
      return { operator: 'lt', threshold: 0.85 };
    case 'throughput':
      return { operator: 'lt', threshold: 1 };
    case 'error_count':
      return { operator: 'gt', threshold: 10 };
    case 'new_error_groups':
      return { operator: 'gt', threshold: 0 };
    case 'request_count':
      return { operator: 'lt', threshold: 10 };
    case 'slo_burn_rate':
      return { operator: 'gt', threshold: 14.4 };
    default:
      return { operator: 'gt', threshold: 1_000 };
  }
}

/** The APM source an APM page asked for through the address, or none. */
export function apmDraftFromSearch(search: string, metrics: readonly ApmAlertMetric[] = APM_METRICS): ApmAlertQuery | null {
  const params = new URLSearchParams(search);
  if (params.get('source') !== 'apm') return null;

  const metric = params.get('metric') as ApmAlertMetric | null;
  const endpoint = params.get('endpoint');
  const kind = params.get('kind') as (typeof APM_TRANSACTION_KINDS)[number] | null;

  return {
    service: params.get('service') ?? '',
    namespace: params.get('namespace') || null,
    environment: params.get('environment') || null,
    transaction: endpoint ? { name: endpoint, kind: kind && APM_TRANSACTION_KINDS.includes(kind) ? kind : 'http' } : null,
    metric: metric && metrics.includes(metric) ? metric : 'error_rate',
  };
}

interface ApmRuleSource {
  namespace: string;
  service: string;
  environment: string;
  endpoint?: { name: string; kind: string } | null;
  metric?: ApmAlertMetric;
}

/** The address of a new rule on an APM figure, as APM pages open it. */
export function apmNewRuleHref(source: ApmRuleSource): string {
  return `/alerts/rules/new?${apmSourceParams(source)}`;
}

/** The address of a new anomaly rule on an APM figure, against the same time on the days before. */
export function apmNewAnomalyHref(source: ApmRuleSource): string {
  return `/alerts/anomaly/add?${apmSourceParams(source)}`;
}

function apmSourceParams(source: ApmRuleSource): URLSearchParams {
  const params = new URLSearchParams({
    source: 'apm',
    namespace: source.namespace,
    service: source.service,
    environment: source.environment,
    metric: source.metric ?? 'error_rate',
  });
  if (source.endpoint) {
    params.set('endpoint', source.endpoint.name);
    params.set('kind', source.endpoint.kind);
  }
  return params;
}

/** The labels an APM rule carries, so its incidents are found again from the service. */
export function apmRuleLabels(draft: ApmAlertQuery): Record<string, string> {
  return {
    'service.name': draft.service.trim(),
    ...(draft.namespace ? { 'service.namespace': draft.namespace } : {}),
    ...(draft.environment ? { 'deployment.environment': draft.environment } : {}),
    ...(draft.transaction ? { 'apm.endpoint': draft.transaction.name } : {}),
  };
}

/** The query of an APM rule: no stream, and a statement the backend writes from the figure. */
export function apmRuleQuery(draft: ApmAlertQuery, periodSecs: number): AlertRule['query'] {
  return {
    language: 'sql',
    statement: '',
    periodSecs: periodSecs,
    stream: null,
    apm: { ...draft, service: draft.service.trim() },
  };
}

/** A figure of a summary in the unit thresholds are set in; rates, latencies and Apdex are `null` without requests. */
export function apmMetricValue(metric: ApmAlertMetric, red: RedSummary, bucketSeconds: number): number | null {
  // Counts and throughput are zero without requests, as the evaluator reads them.
  const counted = metric === 'throughput' || metric === 'request_count' || metric === 'error_count';
  if (red.requestCount === 0 && !counted) return null;
  const millis = (micros?: number | null) => (micros === null || micros === undefined ? null : micros / 1_000);

  switch (metric) {
    case 'error_rate':
      return red.errorRate * 100;
    case 'p50':
      return millis(red.p50Time);
    case 'p90':
      return millis(red.p90Time);
    case 'p95':
      return millis(red.p95Time);
    case 'p99':
      return millis(red.p99Time);
    case 'average':
      return millis(red.durationAverage);
    case 'throughput':
      return red.requestCount / Math.max(1, bucketSeconds);
    case 'request_count':
      return red.requestCount;
    case 'error_count':
      return red.errorCount;
    case 'apdex':
      return red.apdex ?? null;
    case 'new_error_groups':
    case 'slo_burn_rate':
      return null;
  }
}

/** A trend of APM read as the rows of a query, so the preview draws it as any rule's. */
export function apmPreviewResult(trend: readonly TrendPoint[], metric: ApmAlertMetric, bucketSeconds: number, tookMs: number): QueryResult {
  const rows = trend.flatMap((point) => {
    const value = apmMetricValue(metric, point.red, bucketSeconds);
    return value === null ? [] : [[point.bucketAt, value]];
  });

  return { columns: ['_timestamp', 'value'], rows, scannedRows: rows.length, tookMs: tookMs };
}

/** The error groups first seen in each bucket of a trend, read as the rows of a query for the preview. */
export function apmNewErrorsPreview(
  trend: readonly TrendPoint[],
  errors: readonly Pick<ErrorSummary, 'firstSeenAt'>[],
  bucketSeconds: number,
  tookMs: number,
): QueryResult {
  const width = bucketSeconds * 1_000_000;
  const rows = trend.map((point) => [
    point.bucketAt,
    errors.filter((error) => error.firstSeenAt >= point.bucketAt && error.firstSeenAt < point.bucketAt + width).length,
  ]);

  return { columns: ['_timestamp', 'value'], rows, scannedRows: rows.length, tookMs: tookMs };
}
