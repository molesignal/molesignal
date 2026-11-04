import { flattenRecord } from './model';

/** The attributes a log is first told by, as Datadog's reserved attributes: in this order, each once. */
const KEY_MEANINGS: ReadonlyArray<{ key: string; endings: readonly string[] }> = [
  { key: 'service', endings: ['service.name', 'service'] },
  { key: 'host', endings: ['host.name', 'hostname', 'host'] },
  { key: 'env', endings: ['deployment.environment.name', 'deployment.environment', 'env', 'environment'] },
  { key: 'version', endings: ['service.version', 'version'] },
  { key: 'trace_id', endings: ['trace_id', 'traceid', 'trace.id'] },
  { key: 'span_id', endings: ['span_id', 'spanid', 'span.id'] },
];

export interface KeyAttribute {
  /** What the attribute is: `service`, `host`, … */
  key: string;
  /** The field of the record it was found in, as the record names it. */
  field: string;
  value: unknown;
}

/**
 * The record's key attributes: for each meaning, the first field whose name is,
 * or ends after a dot in, one of the meaning's names. A field is read as it
 * is named, never under another name.
 */
export function keyAttributes(record: Record<string, unknown>): KeyAttribute[] {
  const leaves = flattenRecord(record).filter((row) => row.value !== null && row.value !== undefined && row.value !== '');
  const taken = new Set<string>();

  return KEY_MEANINGS.flatMap(({ key, endings }) => {
    for (const ending of endings) {
      const found = leaves.find((row) => {
        const name = row.path.toLowerCase();
        return !taken.has(row.path) && (name === ending || name.endsWith(`.${ending}`));
      });

      if (found) {
        taken.add(found.path);
        return [{ key, field: found.path, value: found.value }];
      }
    }

    return [];
  });
}

/** The trace a log was written in, and the span, where it says. */
export function logTraceOf(record: Record<string, unknown>): { traceId: string; spanId: string | null } | null {
  const attributes = keyAttributes(record);
  const trace = attributes.find((attribute) => attribute.key === 'trace_id');

  if (!trace || typeof trace.value !== 'string' || !/^[0-9a-f]{16,32}$/i.test(trace.value)) return null;

  const span = attributes.find((attribute) => attribute.key === 'span_id');
  return { traceId: trace.value, spanId: typeof span?.value === 'string' ? span.value : null };
}
