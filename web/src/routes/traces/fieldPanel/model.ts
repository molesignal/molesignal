import type { FieldType } from '@/api/streams';
import { formatTraceDurationMs } from '@/viz/trace/duration';

import type { TraceFieldName } from '../fieldQueryModel';

export interface TraceFieldRecord {
  id: string;
  op: string;
  service: string;
  startNs: number;
  durationMs: number;
  spans: number;
  errors: number;
}

export interface TraceTopValue {
  label: string;
  count: number;
}

export const TRACE_TYPE_GLYPH: Record<FieldType, string> = {
  bool: 'B',
  int64: '#',
  float64: '#',
  utf8: 'T',
  timestamp: 'T',
  json: '{}',
};

export const TRACE_TYPE_COLOR: Record<FieldType, string> = {
  bool: 'text-blue-soft',
  int64: 'text-orange-soft',
  float64: 'text-orange-soft',
  utf8: 'text-green-soft',
  timestamp: 'text-green-soft',
  json: 'text-purple-soft',
};

// The trace list endpoint aggregates spans into one row per trace. Only these
// fields have a trustworthy value distribution at this level; arbitrary span
// attributes remain queryable but intentionally do not pretend to have values.
const DISPLAYABLE_TRACE_FIELDS = new Set<string>([
  'trace_id',
  'service.name',
  'name',
  'status_code',
  'duration_ns',
  'duration_ms',
  'span_count',
  'error_count',
]);

export function isTraceFieldDisplayable(name: TraceFieldName): boolean {
  return DISPLAYABLE_TRACE_FIELDS.has(name);
}

export function traceFieldValue(
  trace: TraceFieldRecord,
  field: TraceFieldName,
): string {
  switch (field) {
    case 'trace_id':
      return trace.id;
    case 'service.name':
      return trace.service;
    case 'name':
      return trace.op;
    case 'status_code':
      return trace.errors > 0 ? 'ERROR' : 'OK';
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

export function traceFieldCount(
  traces: TraceFieldRecord[],
  field: TraceFieldName,
): number {
  return traces.reduce(
    (count, trace) => (traceFieldValue(trace, field) ? count + 1 : count),
    0,
  );
}

export function traceFieldTopValues(
  traces: TraceFieldRecord[],
  field: TraceFieldName,
  limit = 5,
): TraceTopValue[] {
  const counts = new Map<string, number>();
  for (const trace of traces) {
    const label = traceFieldValue(trace, field);
    if (!label) continue;
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([label, count]) => ({ label, count }))
    .sort((left, right) => right.count - left.count || left.label.localeCompare(right.label))
    .slice(0, limit);
}
