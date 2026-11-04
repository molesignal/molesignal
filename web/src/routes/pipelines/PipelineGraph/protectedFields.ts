import type { PipelineSignalType } from './model';
import { isRumStream } from './streamName';
import type { ProcessorConfig } from '../processors/catalog';

/**
 * The fields a realtime pipeline keeps as metrics and spans arrive, as the
 * backend names them (`domain/pipeline/protected.rs`): a step that changes one
 * fails the event there.
 */
const PROTECTED: Record<PipelineSignalType, ReadonlySet<string>> = {
  logs: new Set(),
  // Where a profile's object is, what it measures and when it was taken; its service and labels can change.
  profiles: new Set([
    'id',
    'object_key',
    'archived_bytes',
    '_timestamp',
    'profile_type',
    'duration_nanos',
    'sample_count',
    'total_value',
    'unsymbolized',
    'trace_id',
    'span_id',
  ]),
  // Everything of a sample but its labels, the counts and sums OTLP histograms and summaries keep
  // as plain columns, and the labels that say a sample's bucket or quantile.
  metrics: new Set([
    'metric_name',
    'value',
    '_timestamp',
    'metric_kind',
    'metric_description',
    'metric_unit',
    'metric_temporality',
    'metric_monotonic',
    'metric_start_time_unix_nano',
    '__molesignal_exemplar',
    '__molesignal_exemplar_value',
    '__molesignal_exemplar_labels',
    'count',
    'sum',
    'min',
    'max',
    'le',
    'quantile',
  ]),
  // What a span's place in its trace and its timing rest on.
  traces: new Set([
    'trace_id',
    'span_id',
    'parent_span_id',
    'service.name',
    'start_time_unix_nano',
    'end_time_unix_nano',
    'duration_ns',
  ]),
};

/** The parts of a RUM event the application fills itself; the rest is what the RUM pages read. */
const RUM_CUSTOM = ['context', 'usr.extra', 'feature_flags', 'extra'];

/**
 * Whether a realtime pipeline on this signal keeps `field` as it arrives. On
 * a RUM stream (`source`) every field outside the custom parts is kept.
 */
export function isProtectedField(signal: PipelineSignalType, field: string, source = ''): boolean {
  const name = field.trim().replace(/^\./, '');

  if (signal === 'logs' && isRumStream(source)) {
    return !RUM_CUSTOM.some((custom) => name === custom || name.startsWith(`${custom}.`));
  }

  return PROTECTED[signal].has(name);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * The first protected field a step changed, took away or added, comparing
 * the event before it with the event after it; null when it left them all.
 */
export function changedProtectedField(
  signal: PipelineSignalType,
  before: unknown,
  after: unknown,
  source = '',
): string | null {
  if (!isRecord(before) || !isRecord(after)) return null;

  for (const [field, value] of Object.entries(before)) {
    if (!isProtectedField(signal, field, source)) continue;
    if (!(field in after) || JSON.stringify(after[field]) !== JSON.stringify(value)) return field;
  }

  return Object.keys(after).find((field) => isProtectedField(signal, field, source) && !(field in before)) ?? null;
}

/**
 * The fields a processor writes or takes away. A parser that spreads what it
 * finds over the top of the event names none: what it writes is known only
 * when it runs.
 */
export function writtenFields(processor: ProcessorConfig): string[] {
  switch (processor.op) {
    case 'parse_json':
    case 'parse_key_value':
      return [processor.target, ...(processor.removeSource ? [processor.source] : [])].filter(Boolean);
    case 'rename':
      return [processor.from, processor.to];
    case 'rename_regex':
    case 'keep':
      return [processor.within].filter(Boolean);
    case 'remove':
      return processor.fields;
    case 'set':
    case 'mask':
    case 'case':
    case 'convert':
      return [processor.field];
    case 'lookup':
      return processor.fields.map((mapping) => mapping.field);
    default:
      return [];
  }
}
