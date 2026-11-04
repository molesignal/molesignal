import type { CountingKind, CountingParts, MetricSettings, OverAction, StepCondition } from './model';
import { MIB, quotaLimit, wholeCount } from './model';
import type { FilterTest } from '../processors/catalog';

/**
 * What a counting step does with one event in the editor's test: which events
 * its condition picks and what it would count them as, read the way the
 * backend reads them (`domain/pipeline/condition.rs`, `metric.rs`,
 * `limit.rs`). A test counts nothing for real and never goes past a limit.
 */
export type CountingOutcome =
  | { kind: 'not_picked' }
  /** `value` is what the event adds; `null` when the field the metric reads holds no number. */
  | { kind: 'metric'; series: string; type: MetricSettings['type']; field: string; value: number | null }
  | {
      kind: 'limit';
      step: Exclude<CountingKind, 'generate_metric'>;
      /** The values of the fields the limit is split or grouped by; empty for the whole of it. */
      bucket: Array<[string, string | null]>;
      limit: number;
      /** A quota's unit, a throttle's window in seconds. */
      per: 'events' | 'bytes' | number;
      action: OverAction;
      target: string;
    };

/** The field of an event: its key as written, else the path of its dotted parts. */
export function fieldValue(event: unknown, field: string): unknown {
  const own = member(event, field);
  if (own !== undefined || !field.includes('.')) return own;

  return field
    .split('.')
    .map((part) => part.trim())
    .filter(Boolean)
    .reduce<unknown>((value, key) => member(value, key), event);
}

function member(value: unknown, key: string): unknown {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)[key]
    : undefined;
}

/** A number, or text that reads as one. */
export function numberOf(value: unknown): number | null {
  const number =
    typeof value === 'number' ? value : typeof value === 'string' && value.trim() !== '' ? Number(value.trim()) : NaN;

  return Number.isFinite(number) ? number : null;
}

function numberLiteral(text: string): number | null {
  return /^-?\d+(\.\d+)?$/.test(text.trim()) ? Number(text.trim()) : null;
}

function equals(value: unknown, text: string): boolean {
  if (typeof value === 'string') return value === text;
  if (typeof value === 'number') return numberLiteral(text) === value;
  if (typeof value === 'boolean') return text === String(value);
  return false;
}

function holds(test: FilterTest, value: unknown, text: string): boolean {
  const present = value !== undefined;

  switch (test) {
    case 'exists':
      return present;
    case 'missing':
      return !present;
    case 'equals':
      return present && equals(value, text);
    case 'not_equals':
      return !(present && equals(value, text));
    case 'contains':
      return typeof value === 'string' && value.includes(text);
    case 'starts_with':
      return typeof value === 'string' && value.startsWith(text);
    case 'matches':
      try {
        return typeof value === 'string' && new RegExp(text).test(value);
      } catch {
        return false;
      }
    default: {
      const number = numberOf(value);
      const bound = numberLiteral(text);
      if (number === null || bound === null) return false;
      if (test === 'gt') return number > bound;
      if (test === 'gte') return number >= bound;
      if (test === 'lt') return number < bound;
      return number <= bound;
    }
  }
}

/** Whether every test with a field holds for `event`. */
export function conditionsHold(conditions: readonly StepCondition[], event: unknown): boolean {
  return conditions
    .filter((condition) => condition.field.trim() !== '')
    .every((condition) => holds(condition.test, fieldValue(event, condition.field.trim()), condition.value));
}

/** A label or bucket value made of a field's value: text as it is, anything else as JSON. */
function labelValue(value: unknown): string | null {
  if (value === undefined || value === null) return null;
  return typeof value === 'string' ? value : JSON.stringify(value);
}

function named(fields: readonly string[]): string[] {
  return fields.map((field) => field.trim()).filter(Boolean);
}

/** What the step makes of `event`: nothing when its condition does not pick it. */
export function countingOutcome(kind: CountingKind, parts: CountingParts, event: unknown): CountingOutcome {
  if (!conditionsHold(parts.conditions ?? [], event)) return { kind: 'not_picked' };

  if (kind === 'generate_metric' && parts.metric) {
    const metric = parts.metric;
    const labels = named(metric.groupBy).flatMap((label) => {
      const value = labelValue(fieldValue(event, label));
      return value === null ? [] : [`${label}="${value}"`];
    });
    const field = metric.field.trim();

    return {
      kind: 'metric',
      series: `${metric.name.trim()}${labels.length > 0 ? `{${labels.join(', ')}}` : ''}`,
      type: metric.type,
      field,
      value: field === '' ? 1 : numberOf(fieldValue(event, field)),
    };
  }

  if (kind === 'quota' && parts.quota) {
    const quota = parts.quota;
    const bucket = named(quota.partitionBy).map(
      (field): [string, string | null] => [field, labelValue(fieldValue(event, field))],
    );
    const own = quota.overrides.find((override) =>
      bucket.every(([field, value]) => (override.values[field] ?? '') === (value ?? '')),
    );
    const limit = quotaLimit(own?.limit ?? quota.limit, quota.unit) ?? 0;

    return {
      kind: 'limit',
      step: 'quota',
      bucket,
      limit: quota.unit === 'bytes' ? limit / MIB : limit,
      per: quota.unit,
      action: quota.action,
      target: quota.target.trim(),
    };
  }

  const throttle = parts.throttle;

  return {
    kind: 'limit',
    step: 'throttle',
    bucket: named(throttle?.groupBy ?? []).map(
      (field): [string, string | null] => [field, labelValue(fieldValue(event, field))],
    ),
    limit: wholeCount(throttle?.limit ?? '') ?? 0,
    per: wholeCount(throttle?.windowSecs ?? '') ?? 0,
    action: throttle?.action ?? 'drop',
    target: throttle?.target.trim() ?? '',
  };
}
