import { isRumStream, isWritableStream, keepsEveryRow } from '../PipelineGraph/streamName';
import { FILTER_TESTS, filterTestTakesValue, type FilterTest } from '../processors/catalog';

/**
 * Steps of a realtime pipeline that keep count across events: a metric made
 * of the events a step picks, a quota a UTC day and a throttle. They change no
 * field. Each picks its events with field tests that must all hold, the tests
 * of the filter; none picks every event. The backend's rules are in
 * `domain/pipeline/{metric,limit,condition}.rs`.
 */
export type CountingKind = 'generate_metric' | 'quota' | 'throttle';
export const COUNTING_KINDS: readonly CountingKind[] = ['generate_metric', 'quota', 'throttle'];

export interface StepCondition {
  field: string;
  test: FilterTest;
  value: string;
}

export type MetricType = 'count' | 'gauge' | 'histogram';
export const METRIC_TYPES: readonly MetricType[] = ['count', 'gauge', 'histogram'];
/** The intervals a metric may count over, in seconds. */
export const METRIC_INTERVALS: readonly number[] = [10, 30, 60, 300];
export const DEFAULT_METRIC_INTERVAL = 60;
/** What a new histogram's upper bounds start as: a request's milliseconds. */
export const DEFAULT_BUCKETS = ['5', '10', '25', '50', '100', '250', '500', '1000', '2500', '5000', '10000'];
export const MAX_GROUP_BY = 10;
export const MAX_BUCKETS = 30;
export const MAX_PARTITION_FIELDS = 5;
export const MAX_OVERRIDES = 100;

export interface MetricSettings {
  name: string;
  type: MetricType;
  /** The field whose number a gauge or a histogram reads; a count adds it up when one is named. */
  field: string;
  groupBy: string[];
  /** A histogram's upper bounds as typed. */
  buckets: string[];
  stream: string;
  intervalSecs: number;
}

export type QuotaUnit = 'events' | 'bytes';
/** What happens past the limit: a throttle has no `pass`. */
export type OverAction = 'drop' | 'pass' | 'route';

export interface QuotaOverride {
  values: Record<string, string>;
  /** In the form's unit: events, or MiB. */
  limit: string;
}

export interface QuotaSettings {
  unit: QuotaUnit;
  /** A day's limit in the form's unit: events, or MiB, which is saved as bytes. */
  limit: string;
  partitionBy: string[];
  overrides: QuotaOverride[];
  action: OverAction;
  target: string;
}

export interface ThrottleSettings {
  limit: string;
  windowSecs: string;
  groupBy: string[];
  action: Exclude<OverAction, 'pass'>;
  target: string;
}

/** The parts of a step that a counting kind uses. */
export interface CountingParts {
  conditions?: StepCondition[];
  metric?: MetricSettings;
  quota?: QuotaSettings;
  throttle?: ThrottleSettings;
}

export const MIB = 1_048_576;

export function isCountingKind(kind: unknown): kind is CountingKind {
  return kind === 'generate_metric' || kind === 'quota' || kind === 'throttle';
}

/**
 * The counting steps a pipeline can take: only a realtime one does. Metrics are made of logs, RUM
 * events and spans; a quota or a throttle holds back logs and the RUM events a step may drop,
 * since holding back samples or spans would leave gaps (`realtime::validate_config`).
 */
export function countingKinds(mode: string | undefined, signalType: string, source: string): CountingKind[] {
  if (mode !== 'realtime') return [];

  const metric: CountingKind[] = signalType === 'logs' || signalType === 'traces' ? ['generate_metric'] : [];
  const limits: CountingKind[] = signalType === 'logs' && !keepsEveryRow(source.trim()) ? ['quota', 'throttle'] : [];

  return [...metric, ...limits];
}

/** What a new step of a kind starts as. Its metric goes into a metrics stream named like the source. */
export function defaultCounting(kind: CountingKind, source = ''): CountingParts {
  if (kind === 'generate_metric') {
    return {
      conditions: [],
      metric: {
        name: '',
        type: 'count',
        field: '',
        groupBy: [],
        buckets: [],
        stream: source,
        intervalSecs: DEFAULT_METRIC_INTERVAL,
      },
    };
  }
  if (kind === 'quota') {
    return {
      conditions: [],
      quota: { unit: 'events', limit: '', partitionBy: [], overrides: [], action: 'drop', target: '' },
    };
  }

  return {
    conditions: [],
    throttle: { limit: '', windowSecs: '60', groupBy: [], action: 'drop', target: '' },
  };
}

const TEXT = (value: unknown): value is string => typeof value === 'string';

function texts(value: unknown): string[] {
  return Array.isArray(value) ? value.filter(TEXT) : [];
}

function record(value: unknown): Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

/** A number saved by the backend as the form shows it: bytes as MiB. */
function shown(value: unknown, unit: QuotaUnit = 'events'): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '';
  if (unit === 'events') return String(value);

  return String(Number((value / MIB).toFixed(3)));
}

/** A saved step of a counting kind read back; what it does not say reads as the default. */
export function countingOf(kind: CountingKind, raw: Record<string, unknown>): CountingParts {
  const conditions = (Array.isArray(raw.conditions) ? raw.conditions : []).flatMap((item) => {
    const test = record(item);
    return TEXT(test.field) && FILTER_TESTS.includes(test.test as FilterTest)
      ? [{ field: test.field, test: test.test as FilterTest, value: TEXT(test.value) ? test.value : '' }]
      : [];
  });

  if (kind === 'generate_metric') {
    const metric = record(raw.metric);
    const type = METRIC_TYPES.includes(metric.type as MetricType) ? (metric.type as MetricType) : 'count';

    return {
      conditions,
      metric: {
        name: TEXT(metric.name) ? metric.name : '',
        type,
        field: TEXT(metric.field) ? metric.field : '',
        groupBy: texts(metric.groupBy),
        buckets: Array.isArray(metric.buckets)
          ? metric.buckets.filter((bound) => typeof bound === 'number').map(String)
          : [],
        stream: TEXT(metric.stream) ? metric.stream : '',
        intervalSecs:
          typeof metric.intervalSecs === 'number' && METRIC_INTERVALS.includes(metric.intervalSecs)
            ? metric.intervalSecs
            : DEFAULT_METRIC_INTERVAL,
      },
    };
  }

  if (kind === 'quota') {
    const quota = record(raw.quota);
    const unit: QuotaUnit = quota.unit === 'bytes' ? 'bytes' : 'events';

    return {
      conditions,
      quota: {
        unit,
        limit: shown(quota.limit, unit),
        partitionBy: texts(quota.partitionBy),
        overrides: (Array.isArray(quota.overrides) ? quota.overrides : []).map((item) => {
          const override = record(item);
          const values = record(override.values);

          return {
            values: Object.fromEntries(Object.entries(values).filter(([, value]) => TEXT(value))) as Record<
              string,
              string
            >,
            limit: shown(override.limit, unit),
          };
        }),
        action: quota.action === 'pass' || quota.action === 'route' ? quota.action : 'drop',
        target: TEXT(quota.target) ? quota.target : '',
      },
    };
  }

  const throttle = record(raw.throttle);

  return {
    conditions,
    throttle: {
      limit: shown(throttle.limit),
      windowSecs: shown(throttle.windowSecs) || '60',
      groupBy: texts(throttle.groupBy),
      action: throttle.action === 'route' ? 'route' : 'drop',
      target: TEXT(throttle.target) ? throttle.target : '',
    },
  };
}

/** A whole number as typed, from 1 up; `null` for anything else. */
export function wholeCount(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;

  const number = Number(trimmed);
  return number >= 1 && Number.isSafeInteger(number) ? number : null;
}

/** A quota's limit as the backend counts it: events, or the MiB typed as bytes. */
export function quotaLimit(text: string, unit: QuotaUnit): number | null {
  if (unit === 'events') return wholeCount(text);

  const trimmed = text.trim();
  if (!/^\d+(\.\d+)?$/.test(trimmed)) return null;

  const bytes = Math.round(Number(trimmed) * MIB);
  return bytes >= 1 && Number.isSafeInteger(bytes) ? bytes : null;
}

function named(fields: readonly string[]): string[] {
  return fields.map((field) => field.trim()).filter(Boolean);
}

/** The step's own part of what the backend takes, beside its name and kind. */
export function countingInput(kind: CountingKind, parts: CountingParts) {
  const conditions = (parts.conditions ?? [])
    .filter((condition) => condition.field.trim() !== '')
    .map((condition) => ({
      field: condition.field.trim(),
      test: condition.test,
      ...(filterTestTakesValue(condition.test) && { value: condition.value }),
    }));

  if (kind === 'generate_metric') {
    const metric = parts.metric ?? (defaultCounting(kind).metric as MetricSettings);

    return {
      conditions,
      metric: {
        name: metric.name.trim(),
        type: metric.type,
        ...(metric.field.trim() && { field: metric.field.trim() }),
        groupBy: named(metric.groupBy),
        ...(metric.type === 'histogram' && {
          buckets: named(metric.buckets).map(Number),
        }),
        stream: metric.stream.trim(),
        intervalSecs: metric.intervalSecs,
      },
    };
  }

  if (kind === 'quota') {
    const quota = parts.quota ?? (defaultCounting(kind).quota as QuotaSettings);
    const partitionBy = named(quota.partitionBy);

    return {
      conditions,
      quota: {
        unit: quota.unit,
        limit: quotaLimit(quota.limit, quota.unit) ?? 0,
        partitionBy,
        overrides: quota.overrides.map((override) => ({
          values: Object.fromEntries(partitionBy.map((field) => [field, override.values[field] ?? ''])),
          limit: quotaLimit(override.limit, quota.unit) ?? 0,
        })),
        action: quota.action,
        ...(quota.action === 'route' && { target: quota.target.trim() }),
      },
    };
  }

  const throttle = parts.throttle ?? (defaultCounting(kind).throttle as ThrottleSettings);

  return {
    conditions,
    throttle: {
      limit: wholeCount(throttle.limit) ?? 0,
      windowSecs: wholeCount(throttle.windowSecs) ?? 0,
      groupBy: named(throttle.groupBy),
      action: throttle.action,
      ...(throttle.action === 'route' && { target: throttle.target.trim() }),
    },
  };
}

const METRIC_NAME = /^[A-Za-z_:][A-Za-z0-9_:]*$/;
const NUMERIC_TESTS: ReadonlySet<FilterTest> = new Set(['gt', 'gte', 'lt', 'lte']);

/** Fields every sample of a metrics stream has, or that say which bucket or quantile it is. */
const NOT_LABELS = new Set([
  'value',
  '_timestamp',
  'le',
  'quantile',
  'count',
  'sum',
  'min',
  'max',
  'metric_name',
  'metric_kind',
  'metric_description',
  'metric_unit',
  'metric_temporality',
  'metric_monotonic',
  'metric_start_time_unix_nano',
]);

/** Whether a field can become a label of a generated metric. */
export function isLabelField(field: string): boolean {
  return !NOT_LABELS.has(field) && !field.startsWith('__');
}

/** What a counting step still lacks or gets wrong, as a validation code; `null` when it is complete. */
export function countingProblem(kind: CountingKind, parts: CountingParts): string | null {
  // A row without a field is left out; one with a field says what it compares with.
  const badTest = (parts.conditions ?? []).find((condition) => {
    if (condition.field.trim() === '' || !filterTestTakesValue(condition.test)) return false;
    if (condition.value.trim() === '') return true;
    return NUMERIC_TESTS.has(condition.test) && !/^-?\d+(\.\d+)?$/.test(condition.value.trim());
  });
  if (badTest) return 'counting_condition_invalid';

  if (kind === 'generate_metric') {
    const metric = parts.metric;
    if (!metric || metric.name.trim() === '' || metric.stream.trim() === '') return 'processor_field_missing';
    if (!METRIC_NAME.test(metric.name.trim())) return 'metric_name_invalid';
    if (metric.type !== 'count' && metric.field.trim() === '') return 'processor_field_missing';

    const groupBy = named(metric.groupBy);
    if (groupBy.length > MAX_GROUP_BY || new Set(groupBy).size !== groupBy.length) return 'metric_group_invalid';
    if (groupBy.some((field) => !isLabelField(field))) return 'metric_label_reserved';

    if (metric.type === 'histogram') {
      const bounds = named(metric.buckets).map(Number);
      const ascending = bounds.every(
        (bound, index, all) => Number.isFinite(bound) && bound > (all[index - 1] ?? Number.NEGATIVE_INFINITY),
      );
      if (bounds.length === 0 || bounds.length > MAX_BUCKETS || !ascending) return 'metric_buckets_invalid';
    }

    return null;
  }

  if (kind === 'quota') {
    const quota = parts.quota;
    if (!quota || quotaLimit(quota.limit, quota.unit) === null) return 'limit_invalid';

    const partitionBy = named(quota.partitionBy);
    if (partitionBy.length > MAX_PARTITION_FIELDS || new Set(partitionBy).size !== partitionBy.length) {
      return 'limit_fields_invalid';
    }
    if (quota.overrides.length > MAX_OVERRIDES) return 'limit_overrides_invalid';
    if (
      quota.overrides.some(
        (override) =>
          quotaLimit(override.limit, quota.unit) === null ||
          partitionBy.some((field) => (override.values[field] ?? '') === ''),
      )
    ) {
      return 'limit_overrides_invalid';
    }
    if (quota.overrides.length > 0 && partitionBy.length === 0) return 'limit_overrides_invalid';
    const keys = quota.overrides.map((override) => JSON.stringify(partitionBy.map((field) => override.values[field])));
    if (new Set(keys).size !== keys.length) return 'limit_overrides_invalid';
    if (quota.action === 'route' && quota.target.trim() === '') return 'limit_target_missing';

    return null;
  }

  const throttle = parts.throttle;
  const limit = throttle ? wholeCount(throttle.limit) : null;
  const window = throttle ? wholeCount(throttle.windowSecs) : null;
  if (!throttle || limit === null || limit > 1_000_000 || window === null || window > 86_400) return 'limit_invalid';

  const groupBy = named(throttle.groupBy);
  if (groupBy.length > MAX_PARTITION_FIELDS || new Set(groupBy).size !== groupBy.length) return 'limit_fields_invalid';
  if (throttle.action === 'route' && throttle.target.trim() === '') return 'limit_target_missing';

  return null;
}

/** The stream a quota or a throttle sends what goes past its limit to; `null` when it does not. */
export function overLimitTarget(kind: CountingKind, parts: CountingParts): string | null {
  const settings = kind === 'quota' ? parts.quota : kind === 'throttle' ? parts.throttle : undefined;

  return settings?.action === 'route' ? settings.target.trim() : null;
}

/** A query that reads the metric, to copy into a dashboard or an alert rule. */
export function metricQuery(metric: MetricSettings): string {
  const name = metric.name.trim() || 'metric';
  const by = named(metric.groupBy).map((field) => (/^[A-Za-z_][A-Za-z0-9_]*$/.test(field) ? field : `"${field}"`));

  if (metric.type === 'histogram') {
    return `histogram_quantile(0.99, sum by (${['le', ...by].join(', ')}) (rate(${name}_bucket[5m])))`;
  }
  if (metric.type === 'gauge') return name;

  return by.length > 0 ? `sum by (${by.join(', ')}) (increase(${name}[5m]))` : `sum(increase(${name}[5m]))`;
}

/** A problem of a counting step, as the editor's validation shows it. */
export interface CountingIssue {
  code: string;
  params?: Record<string, string>;
  /** Only something not filled in yet. */
  pending?: true;
}

/** Codes of `countingProblem` that say something is not filled in yet, not that it is wrong. */
const PENDING = new Set(['processor_field_missing', 'limit_target_missing']);

/**
 * What is wrong with a counting step in a pipeline that can take `allowed`
 * and reads `source`; `null` when nothing is.
 */
export function countingIssue(
  kind: CountingKind,
  parts: CountingParts,
  allowed: readonly CountingKind[],
  source: string,
  mode: string | undefined,
): CountingIssue | null {
  if (!allowed.includes(kind)) {
    return { code: mode === 'realtime' ? `counting_not_here_${kind === 'generate_metric' ? 'metric' : 'limit'}` : 'counting_realtime_only' };
  }

  const problem = countingProblem(kind, parts);
  if (problem) return { code: problem, ...(PENDING.has(problem) && { pending: true }) };

  const stream = parts.metric?.stream.trim() ?? '';
  if (kind === 'generate_metric' && !isWritableStream(stream)) return { code: 'metric_stream_unwritable', params: { stream } };

  const target = overLimitTarget(kind, parts);
  if (target === null) return null;
  if (isRumStream(source.trim())) return { code: 'in_place_route' };
  if (target === source.trim()) return { code: 'limit_target_source', params: { stream: target } };
  if (!isWritableStream(target)) {
    return { code: isRumStream(target) ? 'route_to_rum' : 'route_to_platform', params: { stream: target } };
  }

  return null;
}
