import type { TFunction } from 'i18next';

import type { MetricCatalogEntry } from '@/api/metricsCatalog';
import type * as queryApi from '@/api/query';
import type { CodeCompletionItem } from '@/shell/codeEditor/types';
import type { GlobalFilter } from '@/stores/useFiltersStore';
import type { TimeWindow } from '@/stores/useTimeStore';

export type MetricsDrawStyle = 'line' | 'bar' | 'points';
export type MetricsStackMode = 'none' | 'normal' | 'percent';

export function injectPromqlMatchers(
  promql: string,
  filters: GlobalFilter[],
): string {
  const valid = filters.filter(
    (filter) =>
      /^[a-zA-Z_][a-zA-Z0-9_]*$/.test(filter.key) && filter.value,
  );
  if (valid.length === 0) return promql;

  const trimmed = promql.trim();
  const rate = trimmed.match(
    /^(rate|irate|increase)\(\s*([a-zA-Z_:][\w:]*)(\{[^}]*\})?\s*\[([^\]]+)\]\s*\)$/,
  );
  if (rate) {
    const [, fn, metric, existing, range] = rate;
    return `${fn}(${metric}${mergePromqlLabels(existing, valid)}[${range}])`;
  }

  const bare = trimmed.match(/^([a-zA-Z_:][\w:]*)(\{[^}]*\})?$/);
  if (bare) {
    const [, metric, existing] = bare;
    return `${metric}${mergePromqlLabels(existing, valid)}`;
  }
  return promql;
}

function mergePromqlLabels(
  existing: string | undefined,
  filters: GlobalFilter[],
): string {
  const inner = existing ? existing.slice(1, -1).trim() : '';
  const present = new Set(
    [...inner.matchAll(/([a-zA-Z_][a-zA-Z0-9_]*)\s*(?:=~|!~|=|!=)/g)].map(
      (match) => match[1],
    ),
  );
  const added = filters
    .filter((filter) => !present.has(filter.key))
    .map(
      (filter) =>
        `${filter.key}${filter.operator === '!=' ? '!=' : '='}"${filter.value
          .replace(/\\/g, '\\\\')
          .replace(/"/g, '\\"')}"`,
    );
  const parts = [inner, ...added].filter(Boolean);
  return parts.length > 0 ? `{${parts.join(',')}}` : '';
}

export function buildPromqlCompletionItems(
  capabilities: queryApi.PromqlCapabilities | undefined,
  metrics: MetricCatalogEntry[],
): CodeCompletionItem[] {
  const kindRank: Record<queryApi.PromqlCapabilityKind, number> = {
    function: 1,
    aggregation: 2,
    keyword: 4,
    operator: 5,
  };
  const capabilityItems = capabilities
    ? (['functions', 'aggregations', 'keywords', 'operators'] as const).flatMap(
        (key) =>
          Array.isArray(capabilities[key]) ? capabilities[key] : [],
      )
    : [];
  const engineItems = capabilityItems.map<CodeCompletionItem>((item) => ({
    label: item.label,
    insertText: item.insert_text,
    insertTextFormat: 'snippet',
    detail: item.detail,
    documentation: item.documentation,
    kind: item.kind,
    sortText: `${kindRank[item.kind]}:${item.label}`,
  }));
  const metricItems = metrics.map<CodeCompletionItem>((metric) => ({
    label: metric.name,
    insertText: metric.name,
    kind: 'metric',
    sortText: `0:${metric.name}`,
    detail: `metric · ${metric.labels.length} labels`,
    documentation:
      metric.labels.length > 0
        ? `Available labels: ${metric.labels.join(', ')}`
        : 'Metric from the current workspace catalog.',
  }));
  const labels = [...new Set(metrics.flatMap((metric) => metric.labels))].sort();
  const labelItems = labels.map<CodeCompletionItem>((label) => ({
    label,
    insertText: label,
    kind: 'label',
    sortText: `3:${label}`,
    detail: 'metric label',
    documentation: 'Label available in the current workspace metric catalog.',
  }));
  return [...metricItems, ...engineItems, ...labelItems];
}

export function requestedPromqlFromParams(params: URLSearchParams): string {
  const direct = params.get('promql') ?? params.get('q') ?? '';
  if (direct.trim()) return direct.trim();
  const metric = params.get('metric')?.trim();
  const service = params.get('service')?.trim();
  const host = params.get('host')?.trim();
  const instance = params.get('instance')?.trim();
  const traceId =
    params.get('trace_id')?.trim() ?? params.get('traceId')?.trim();
  if (metric) return metric;

  const matchers: Record<string, string> = {};
  if (service) matchers.service = service;
  if (host) matchers.host = host;
  if (instance) matchers.instance = instance;
  if (traceId) matchers.trace_id = traceId;
  const matcherExpr = Object.entries(matchers)
    .map(
      ([key, value]) =>
        `${key}="${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`,
    )
    .join(',');
  return matcherExpr
    ? `rate(http_requests_total{${matcherExpr}}[5m])`
    : '';
}

export function metricChartTitle(
  metricName: string | null,
  expression: string,
  t: TFunction<'metrics'>,
): string {
  const rate = isRateQuery(expression);
  if (rate && metricName === 'http_requests_total') {
    return t('explore.chart.http_request_rate');
  }
  if (!metricName) return t('explore.chart.query_result');
  const readable = metricName
    .replace(/^_+/, '')
    .replace(/_+/g, ' ')
    .replace(/\bhttp\b/gi, 'HTTP')
    .replace(/\bapi\b/gi, 'API');
  return rate
    ? t('explore.chart.generic_rate', { metric: readable })
    : readable;
}

export function metricQueryUnit(
  expression: string,
  metricName: string | null,
): string | undefined {
  const normalized = metricName?.toLowerCase() ?? '';
  if (isRateQuery(expression)) {
    if (/request/.test(normalized)) return 'req/s';
    if (/byte/.test(normalized)) return 'B/s';
    return 'ops/s';
  }
  if (/(?:^|_)bytes?(?:_|$)/.test(normalized)) return 'bytes';
  if (/(?:duration|latency).*_ms$/.test(normalized)) return 'ms';
  if (/(?:percent|percentage)$/.test(normalized)) return 'percent';
  return undefined;
}

export function isRateQuery(expression: string): boolean {
  return /\b(?:rate|irate)\s*\(/i.test(expression);
}

export function formatMetricDuration(
  seconds: number,
  language: string,
): string {
  const value = Math.max(0, seconds);
  const zh = language.toLowerCase().startsWith('zh');
  if (value < 1) return `${Math.round(value * 1000)} ms`;
  if (value < 60) return `${formatDurationNumber(value)} ${zh ? '秒' : 's'}`;
  if (value < 3600) {
    return `${formatDurationNumber(value / 60)} ${zh ? '分钟' : 'min'}`;
  }
  if (value < 86_400) {
    return `${formatDurationNumber(value / 3600)} ${zh ? '小时' : 'h'}`;
  }
  return `${formatDurationNumber(value / 86_400)} ${zh ? '天' : 'd'}`;
}

function formatDurationNumber(value: number): string {
  return value >= 10 || Number.isInteger(value)
    ? value.toFixed(0)
    : value.toFixed(1).replace(/\.0$/, '');
}

export function formatPercent(value: number): string {
  return new Intl.NumberFormat(undefined, {
    style: 'percent',
    maximumFractionDigits: 1,
  }).format(value);
}

export function findMetricName(
  expression: string,
  metrics: MetricCatalogEntry[],
): string | null {
  const byLongestName = [...metrics].sort(
    (left, right) => right.name.length - left.name.length,
  );
  return (
    byLongestName.find((metric) => {
      const escaped = metric.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      return new RegExp(
        `(^|[^a-zA-Z0-9_:])${escaped}([^a-zA-Z0-9_:]|$)`,
      ).test(expression);
    })?.name ?? null
  );
}

export function defaultPromqlForMetric(metric: MetricCatalogEntry): string {
  return isCounterMetric(metric) ? `rate(${metric.name}[5m])` : metric.name;
}

function isCounterMetric(metric: MetricCatalogEntry): boolean {
  const name = metric.name.toLowerCase();
  return /(_total|_count|_sum|_bucket)$/.test(name);
}

export function timestampsForSeries(
  timestamps: number[],
  count: number,
  domain: [number, number],
): number[] {
  if (
    timestamps.length === count &&
    timestamps.every(
      (timestamp) => Number.isFinite(timestamp) && timestamp > 0,
    )
  ) {
    return timestamps;
  }
  return stretchTimestampsToWindow(count, domain);
}

export function timeWindowKey(window: TimeWindow): string {
  return `${window.mode}:${window.from}:${window.to}`;
}

function stretchTimestampsToWindow(
  count: number,
  [from, to]: [number, number],
): number[] {
  if (count <= 0) return [];
  if (count === 1) return [to];
  const span = Math.max(to - from, 1);
  return Array.from(
    { length: count },
    (_, index) => from + (span * index) / (count - 1),
  );
}
