import { renderLegendFormat } from '@/dashboard-engine/dataframe';
import {
  QUERY_LEGEND_AUTO,
  resolveQueryLegendMode,
} from '@/dashboard-engine/query/legend';
import {
  grafanaMetricSeriesName,
  type MetricSeries,
} from '@/lib/metricsSeries';

export type MetricsResultFormat = 'time_series' | 'table';
export type MetricsQueryType = 'range' | 'instant';

export interface MetricsQueryOptions {
  legend: string | undefined;
  format: MetricsResultFormat;
  step: string;
  type: MetricsQueryType;
  exemplars: boolean;
}

export const DEFAULT_METRICS_QUERY_OPTIONS: MetricsQueryOptions = {
  legend: QUERY_LEGEND_AUTO,
  format: 'time_series',
  step: 'auto',
  type: 'range',
  exemplars: true,
};

const AUTO_RANGE_LIMIT = 1_000;
const STEP_UNIT_MILLISECONDS: Record<string, number> = {
  ms: 1,
  s: 1_000,
  m: 60_000,
  h: 3_600_000,
  d: 86_400_000,
  w: 604_800_000,
  y: 31_536_000_000,
};

/** Parse Prometheus-style durations such as `30s`, `5m`, or `1h30m`. */
export function parseMetricsStepMilliseconds(value: string): number | null {
  const normalized = value.trim().toLowerCase();
  if (!normalized || normalized === 'auto') return null;

  const part = /(\d+(?:\.\d+)?)(ms|s|m|h|d|w|y)/gy;
  let total = 0;
  let consumed = 0;
  for (const match of normalized.matchAll(part)) {
    if (match.index !== consumed) return null;
    const amount = Number(match[1]);
    const multiplier = STEP_UNIT_MILLISECONDS[match[2]!];
    if (!Number.isFinite(amount) || amount <= 0 || multiplier === undefined) {
      return null;
    }
    total += amount * multiplier;
    consumed += match[0].length;
  }
  return consumed === normalized.length && total > 0 ? total : null;
}

export function isValidMetricsStep(value: string): boolean {
  const normalized = value.trim().toLowerCase();
  return normalized === '' || normalized === 'auto' || parseMetricsStepMilliseconds(value) !== null;
}

/**
 * The current PromQL API derives its range step from `time span / limit`.
 * Translate the editable Explore step into that existing contract. `limit=1`
 * selects the engine's instant-evaluation path.
 */
export function metricsQueryLimit(
  options: MetricsQueryOptions,
  startMicros: number,
  endMicros: number,
): number {
  if (options.type === 'instant') return 1;
  const stepMilliseconds = parseMetricsStepMilliseconds(options.step);
  if (stepMilliseconds === null) return AUTO_RANGE_LIMIT;
  const spanMilliseconds = Math.max((endMicros - startMicros) / 1_000, 1);
  return Math.min(
    AUTO_RANGE_LIMIT,
    Math.max(2, Math.ceil(spanMilliseconds / stepMilliseconds)),
  );
}

/** Match Dashboard's Grafana-compatible Auto, Verbose, and Custom modes. */
export function metricsLegendNames(
  series: MetricSeries[],
  metricName: string | undefined,
  legend: string | undefined,
): string[] {
  const mode = resolveQueryLegendMode(legend);
  return series.map((item) => {
    if (mode === 'custom') {
      return renderLegendFormat(legend!, item.labels);
    }
    if (mode === 'auto') {
      return grafanaMetricSeriesName(item, metricName);
    }
    return formatDashboardLegendLabels(
      item.labels,
      metricName?.trim() || item.valueColumn,
    );
  });
}

export function metricsLegendTitles(
  series: MetricSeries[],
  metricName: string | undefined,
): string[] {
  return series.map((item) => grafanaMetricSeriesName(item, metricName));
}

function formatDashboardLegendLabels(
  labels: Record<string, string>,
  fallback: string,
): string {
  const entries = Object.entries(labels).sort(([left], [right]) =>
    left.localeCompare(right),
  );
  if (entries.length === 0) return fallback;
  return `{${entries
    .map(([key, value]) => `${key}=${JSON.stringify(value)}`)
    .join(', ')}}`;
}
