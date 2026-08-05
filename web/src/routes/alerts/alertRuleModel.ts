import type {
  AlertRule,
  ComparisonOp,
  Severity,
  SeverityThreshold,
} from '@/types/alerting';
import type { QueryResult } from '@/types/query';

export const SEVERITY_ORDER: Severity[] = ['info', 'warning', 'error', 'critical'];

export const COMPARISON_LABEL: Record<ComparisonOp, string> = {
  gt: '>',
  gte: '≥',
  lt: '<',
  lte: '≤',
  eq: '=',
  neq: '≠',
};

export function severityRank(severity: Severity): number {
  const index = SEVERITY_ORDER.indexOf(severity);
  return index < 0 ? 1 : index;
}

export function topThreshold(rule: AlertRule): SeverityThreshold | null {
  const bands = rule.thresholds ?? [];
  if (bands.length === 0) return null;
  return bands.reduce((top, band) =>
    severityRank(band.severity) > severityRank(top.severity) ? band : top,
  );
}

export function ruleSeverity(rule: AlertRule): Severity {
  const top = topThreshold(rule);
  if (top) return top.severity;
  if (rule.severity) return rule.severity;
  const label = rule.labels?.severity as Severity | undefined;
  return label && SEVERITY_ORDER.includes(label) ? label : 'warning';
}

export function seedThresholds(rule?: AlertRule): SeverityThreshold[] {
  if (rule?.thresholds?.length) return rule.thresholds.map((band) => ({ ...band }));
  if (rule?.trigger) {
    return [
      {
        severity: ruleSeverity(rule),
        operator: rule.trigger.operator,
        threshold: rule.trigger.threshold,
        for_periods: rule.trigger.for_periods,
      },
    ];
  }
  return [{ severity: 'warning', operator: 'gt', threshold: 0.05, for_periods: 5 }];
}

export function formatDurationSecs(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
  return `${Math.round(seconds / 3600)}h`;
}

export interface QueryPoint {
  timestamp?: number;
  value: number;
}

function numeric(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string' && value.trim()) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

function timestampMicros(value: unknown): number | undefined {
  const parsed = numeric(value);
  if (parsed === null) return undefined;
  const absolute = Math.abs(parsed);
  if (absolute < 100_000_000_000) return parsed * 1_000_000;
  if (absolute < 100_000_000_000_000) return parsed * 1_000;
  if (absolute < 100_000_000_000_000_000) return parsed;
  return parsed / 1_000;
}

/**
 * Convert an arbitrary query response into one preview series. The query
 * service can return SQL or PromQL columns, so column names are hints rather
 * than a hard contract.
 */
export function extractQueryPoints(result: QueryResult): QueryPoint[] {
  const columns = result.columns.map((column) => column.toLowerCase());
  const timeIndex = columns.findIndex((column) =>
    ['_timestamp', 'timestamp', 'time', 'ts', 'evaluated_at'].includes(column),
  );
  const preferredValueIndex = columns.findIndex((column) =>
    ['value', 'metric_value', 'result', 'count', 'avg', 'p95', 'rate'].includes(column),
  );

  return result.rows
    .map((row): QueryPoint | null => {
      let valueIndex = preferredValueIndex;
      let value = valueIndex >= 0 ? numeric(row[valueIndex]) : null;
      if (value === null) {
        valueIndex = row.findIndex((cell, index) => index !== timeIndex && numeric(cell) !== null);
        value = valueIndex >= 0 ? numeric(row[valueIndex]) : null;
      }
      if (value === null) return null;
      const point: QueryPoint = { value };
      if (timeIndex >= 0) {
        const timestamp = timestampMicros(row[timeIndex]);
        if (timestamp !== undefined) point.timestamp = timestamp;
      }
      return point;
    })
    .filter((point): point is QueryPoint => point !== null)
    .sort((left, right) => (left.timestamp ?? 0) - (right.timestamp ?? 0));
}

export function conditionMatches(
  value: number,
  operator: ComparisonOp,
  threshold: number,
): boolean {
  if (operator === 'gt') return value > threshold;
  if (operator === 'gte') return value >= threshold;
  if (operator === 'lt') return value < threshold;
  if (operator === 'lte') return value <= threshold;
  if (operator === 'eq') return value === threshold;
  return value !== threshold;
}

/**
 * Estimate distinct firing episodes from historical samples. An episode
 * starts when the configured consecutive-period requirement is first met and
 * ends when the signal returns inside the threshold.
 */
export function estimateTriggerEpisodes(
  values: number[],
  band: SeverityThreshold,
): number {
  let consecutive = 0;
  let active = false;
  let episodes = 0;
  const required = Math.max(1, band.for_periods);
  for (const value of values) {
    if (conditionMatches(value, band.operator, band.threshold)) {
      consecutive += 1;
      if (!active && consecutive >= required) {
        active = true;
        episodes += 1;
      }
    } else {
      consecutive = 0;
      active = false;
    }
  }
  return episodes;
}

export function thresholdConflict(thresholds: SeverityThreshold[]): boolean {
  const sorted = thresholds
    .slice()
    .sort((left, right) => severityRank(left.severity) - severityRank(right.severity));
  for (let index = 1; index < sorted.length; index += 1) {
    const lower = sorted[index - 1]!;
    const higher = sorted[index]!;
    const increasing =
      ['gt', 'gte'].includes(lower.operator) && ['gt', 'gte'].includes(higher.operator);
    const decreasing =
      ['lt', 'lte'].includes(lower.operator) && ['lt', 'lte'].includes(higher.operator);
    if (increasing && higher.threshold < lower.threshold) return true;
    if (decreasing && higher.threshold > lower.threshold) return true;
  }
  return false;
}

export function renderNotificationTemplate(
  body: string,
  context: {
    name: string;
    service: string;
    severity: Severity;
    value?: number;
    threshold?: number;
  },
): string {
  return body
    .replaceAll('{{rule.name}}', context.name || 'Untitled rule')
    .replaceAll('{{incident.summary}}', context.name || 'Untitled rule')
    .replaceAll('{{severity}}', context.severity)
    .replaceAll('{{labels.service}}', context.service || 'unknown-service')
    .replaceAll('{{value}}', context.value === undefined ? '—' : String(context.value))
    .replaceAll(
      '{{threshold}}',
      context.threshold === undefined ? '—' : String(context.threshold),
    );
}
