import type { Span } from '@/api/web';
import type { SignalReferenceTime } from '@/shell/SignalReference';

const TRACE_CONTEXT_PADDING_MS = 5 * 60 * 1000;

/**
 * A reference that reads as the name it is until the pointer or the keyboard is on it: a column of
 * spans, each with an underline, reads as a wall of links.
 */
export const QUIET_REFERENCE = 'no-underline hover:underline focus-visible:underline';

/**
 * About how wide a duration is written after its bar: its characters at the
 * size of the axis's figures, a little over, and the room either side of it.
 */
export function durationLabelWidth(characters: number): number {
  return Math.ceil(characters * 7) + 12;
}

export function traceContextWindow(traceStartNs: number, totalNs: number): SignalReferenceTime | undefined {
  const startMs = traceStartNs / 1_000_000;
  const endMs = (traceStartNs + totalNs) / 1_000_000;
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || startMs <= 0 || endMs < startMs) {
    return undefined;
  }
  const from = new Date(startMs - TRACE_CONTEXT_PADDING_MS);
  const to = new Date(endMs + TRACE_CONTEXT_PADDING_MS);
  if (Number.isNaN(from.getTime()) || Number.isNaN(to.getTime())) return undefined;
  return { from: from.toISOString(), to: to.toISOString() };
}

export function signalLabelsForTrace(traceId: string, span: Span): Record<string, string> {
  return {
    ...stringSignalAttributes(span.attributes),
    trace_id: traceId,
    service_name: span.service,
    operation_name: span.operation,
  };
}

export function signalLabelsForSpan(traceId: string, span: Span): Record<string, string> {
  return {
    ...signalLabelsForTrace(traceId, span),
    span_id: span.spanId,
  };
}

function stringSignalAttributes(
  attributes: Record<string, unknown>,
  prefix = '',
  depth = 0,
): Record<string, string> {
  const labels: Record<string, string> = {};
  for (const [key, value] of Object.entries(attributes)) {
    const qualifiedKey = prefix ? `${prefix}.${key}` : key;
    if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
      labels[qualifiedKey] = String(value);
      continue;
    }
    if (depth < 1 && value && typeof value === 'object' && !Array.isArray(value)) {
      Object.assign(
        labels,
        stringSignalAttributes(value as Record<string, unknown>, qualifiedKey, depth + 1),
      );
    }
  }
  return labels;
}
