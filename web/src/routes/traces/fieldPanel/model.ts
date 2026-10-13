import type { FieldType } from '@/api/streams';
import type { FieldTypeTone } from '@/shell/query/fields/TypeChip';
import { formatTraceDurationMs, formatTraceDurationNs } from '@/viz/trace/duration';

import { isTraceLevelField, type TraceFieldDef, type TraceFieldName } from '../fieldQueryModel';

/** A trace as the list loads it: one row per trace, with what the list works out for it. */
export interface TraceFieldRecord {
  id: string;
  op: string;
  service: string;
  startNs: number;
  durationMs: number;
  spans: number;
  errors: number;
}

export const TRACE_TYPE_GLYPH: Record<FieldType, string> = {
  bool: 'B',
  int64: '#',
  float64: '#',
  utf8: 'T',
  timestamp: 'T',
  json: '{}',
};

export const TRACE_TYPE_TONE: Record<FieldType, FieldTypeTone> = {
  bool: 'blue',
  int64: 'orange',
  float64: 'orange',
  utf8: 'green',
  timestamp: 'green',
  json: 'purple',
};

/**
 * The field's value for one trace, as its row in the list has it; empty when
 * the row has none. A trace's row says nothing of what its spans hold, so
 * only these few fields can be shown beside a trace that is picked.
 */
export function traceFieldValue(trace: TraceFieldRecord, field: TraceFieldName): string {
  switch (field) {
    case 'trace_id':
      return trace.id;
    case 'service.name':
      return trace.service;
    case 'name':
      return trace.op;
    case 'duration_ns':
    case 'duration_ms':
      return formatTraceDurationMs(trace.durationMs);
    case 'span_count':
      return String(trace.spans);
    case 'error_count':
      return String(trace.errors);
    default:
      return '';
  }
}

const LIST_FIELDS = new Set(['trace_id', 'service.name', 'name', 'duration_ns', 'duration_ms', 'span_count', 'error_count']);

/** Whether a trace's own row in the list has a value for the field. */
export function isTraceListField(name: TraceFieldName): boolean {
  return LIST_FIELDS.has(name);
}

/**
 * Whether a field's values can be listed. A JSON column holds a document per
 * span: its "values" would be whole documents, which nobody filters by. A
 * field of a span has to be read from a stream; what a trace has one of is on
 * the loaded rows, and needs none.
 */
export function isTraceFieldExpandable(field: TraceFieldDef, canReadSpans: boolean): boolean {
  return field.dataType !== 'json' && (canReadSpans || isTraceLevelField(field.name));
}

/** One value of a field among the loaded traces, as a filter takes it. */
export interface TraceFieldValue {
  value: string;
  /** How many of the loaded traces have it. */
  traces: number;
  /** In how many of their spans; absent for a field a trace has one value for. */
  spans?: number;
}

/** What the loaded traces hold for a field. */
export interface TraceFieldValues {
  /** How many of the loaded traces have any value for it. */
  traces: number;
  /** How many different values there are; `top` lists the most common of them. */
  distinct: number;
  /** How many times the field was read: spans, or traces for a field a trace has one value for. */
  samples: number;
  top: TraceFieldValue[];
  /** Of a number: what it runs from and to, kept as written so nothing is rounded away. */
  range?: { min: string; max: string; avg: string };
}

/** How many of a field's values are listed. */
export const TRACE_FIELD_VALUE_LIMIT = 8;

export const NO_TRACE_FIELD_VALUES: TraceFieldValues = { traces: 0, distinct: 0, samples: 0, top: [] };

/**
 * The value of a field a trace has one of, as a filter takes it: the stored
 * form, not the one shown. Empty when the row has none.
 */
export function traceLevelValue(trace: TraceFieldRecord, field: TraceFieldName): string {
  if (field === 'trace_id') return trace.id;
  if (field === 'span_count') return String(trace.spans);
  if (field === 'error_count') return String(trace.errors);
  // The list gives a trace's duration in milliseconds; a filter takes it in nanoseconds.
  return Number.isFinite(trace.durationMs) ? String(Math.round(trace.durationMs * 1_000_000)) : '';
}

/**
 * The values of a field a trace has one value for: its id, its duration, its
 * two counts. They are on the loaded rows themselves, so nothing is fetched.
 * Equally common values keep the order of the list.
 */
export function traceLevelFieldValues(
  traces: readonly TraceFieldRecord[],
  field: TraceFieldName,
): TraceFieldValues {
  const counts = new Map<string, number>();

  for (const trace of traces) {
    const value = traceLevelValue(trace, field);
    if (value) counts.set(value, (counts.get(value) ?? 0) + 1);
  }

  const samples = [...counts.values()].reduce((total, count) => total + count, 0);
  const numbers = field === 'trace_id' ? [] : [...counts.entries()].flatMap(([value, count]) => Array<number>(count).fill(Number(value)));

  return {
    traces: samples,
    distinct: counts.size,
    samples,
    top: [...counts.entries()]
      .map(([value, count]) => ({ value, traces: count }))
      .sort((left, right) => right.traces - left.traces)
      .slice(0, TRACE_FIELD_VALUE_LIMIT),
    ...(numbers.length > 0 && {
      range: {
        min: String(Math.min(...numbers)),
        max: String(Math.max(...numbers)),
        avg: String(Math.round(numbers.reduce((total, value) => total + value, 0) / numbers.length)),
      },
    }),
  };
}

/** Whether the field's values are on the loaded rows, and need no query. */
export function hasTraceLevelValues(field: TraceFieldDef): boolean {
  return isTraceLevelField(field.name);
}

/**
 * Whether a number is a measure, read as a range, or a code, read as a list.
 * A measure is nearly never the same twice: there are more values than the
 * list shows, and close to one for every time the field was read. A status
 * code with nine values over four hundred spans is still a list.
 */
export function isTraceFieldMeasure(values: TraceFieldValues): boolean {
  return values.range !== undefined
    && values.distinct > values.top.length
    && values.distinct * 4 >= values.samples;
}

/** What a field's values are, as far as its name and type say: how they are shown, not how they are stored. */
export type TraceValueKind = 'duration_ns' | 'duration_ms' | 'time_ns' | 'time_micros' | 'span_kind' | 'plain';

export function traceValueKind(field: Pick<TraceFieldDef, 'name' | 'dataType'>): TraceValueKind {
  if (field.dataType === 'timestamp') return 'time_micros';
  if (field.dataType !== 'int64' && field.dataType !== 'float64') return 'plain';
  if (/_unix_nano$/.test(field.name)) return 'time_ns';
  if (/(^|[._])duration_ns$/.test(field.name)) return 'duration_ns';
  if (/_ms$/.test(field.name)) return 'duration_ms';
  return field.name === 'kind' || field.name === 'span_kind' ? 'span_kind' : 'plain';
}

/** Whether the values are moments. A moment has an earliest and a latest, and no average. */
export function isTraceTimeKind(kind: TraceValueKind): boolean {
  return kind === 'time_ns' || kind === 'time_micros';
}

// The span kinds of OpenTelemetry, by the number they are stored as.
const SPAN_KINDS = ['UNSPECIFIED', 'INTERNAL', 'SERVER', 'CLIENT', 'PRODUCER', 'CONSUMER'];

/**
 * A value as a person reads it. The stored form is what a filter takes; this
 * stands in its place on the screen: a duration in its natural unit, a moment
 * as a time, the name of a span kind beside its number.
 */
export function describeTraceValue(
  kind: TraceValueKind,
  value: string,
  formatMicros: (micros: number) => string,
): string {
  const number = Number(value);
  if (kind === 'plain' || value.trim() === '' || !Number.isFinite(number)) return value;

  switch (kind) {
    case 'duration_ns':
      return formatTraceDurationNs(number);
    case 'duration_ms':
      return formatTraceDurationMs(number);
    case 'time_ns':
      return formatMicros(number / 1_000);
    case 'time_micros':
      return formatMicros(number);
    case 'span_kind':
      return SPAN_KINDS[number] ? `${value} · ${SPAN_KINDS[number]}` : value;
  }
}
