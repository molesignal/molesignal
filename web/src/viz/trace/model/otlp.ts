import type { Span, TraceResponse } from '@/api/web';

import { spanKindName, splitAttributes } from './span';

/** A value as OTLP/JSON writes it: one field for its type, 64-bit integers as decimal strings. */
export type OtlpAnyValue =
  | Record<string, never>
  | { stringValue: string }
  | { boolValue: boolean }
  | { intValue: string }
  | { doubleValue: number }
  | { arrayValue: { values: OtlpAnyValue[] } }
  | { kvlistValue: { values: OtlpKeyValue[] } };

export interface OtlpKeyValue {
  key: string;
  value: OtlpAnyValue;
}

export interface OtlpSpan {
  traceId: string;
  spanId: string;
  parentSpanId?: string;
  traceState: string;
  flags: number;
  name: string;
  kind: number;
  startTimeUnixNano: string;
  endTimeUnixNano: string;
  attributes: OtlpKeyValue[];
  droppedAttributesCount: number;
  events: Array<{ timeUnixNano: string; name: string; attributes: OtlpKeyValue[]; droppedAttributesCount: number }>;
  droppedEventsCount: number;
  links: Array<{
    traceId: string;
    spanId: string;
    traceState: string;
    flags: number;
    attributes: OtlpKeyValue[];
    droppedAttributesCount: number;
  }>;
  droppedLinksCount: number;
  status: { code: number; message?: string };
}

export interface OtlpScopeSpans {
  scope: { name: string; version: string; attributes: OtlpKeyValue[]; droppedAttributesCount: number };
  schemaUrl?: string;
  spans: OtlpSpan[];
}

export interface OtlpResourceSpans {
  resource: { attributes: OtlpKeyValue[]; droppedAttributesCount: number };
  schemaUrl?: string;
  scopeSpans: OtlpScopeSpans[];
}

/** A trace as an OTLP/JSON export request: what a collector or this server takes in again. */
export interface OtlpTrace {
  resourceSpans: OtlpResourceSpans[];
}

/** OTLP's numbers for a span's kind, in the order the protocol lists them. */
const KINDS = ['unspecified', 'internal', 'server', 'client', 'producer', 'consumer'] as const;

/**
 * A trace's spans as OTLP/JSON: spans of one resource together, and within
 * it those of one instrumentation scope, in the order the trace held them.
 * A span keeps its own tags; the ones the store copied in from its resource
 * go back to the resource, which always names its service. Times are epoch
 * nanoseconds as decimal strings, a span's end its start plus its duration.
 */
export function toOtlpTrace(trace: TraceResponse): OtlpTrace {
  const resources = new Map<string, { entry: OtlpResourceSpans; scopes: Map<string, OtlpScopeSpans> }>();

  for (const span of trace.spans ?? []) {
    const { attributes, resource } = splitAttributes(span);
    const resourceAttributes = Object.fromEntries(resource);
    if (!('service.name' in resourceAttributes) && span.service) resourceAttributes['service.name'] = span.service;

    const resourceKey = JSON.stringify([span.resource?.schema_url ?? '', sortedEntries(resourceAttributes)]);
    let group = resources.get(resourceKey);
    if (!group) {
      group = {
        entry: {
          resource: { attributes: keyValues(resourceAttributes), droppedAttributesCount: span.resource?.dropped_attributes_count ?? 0 },
          ...(span.resource?.schema_url ? { schemaUrl: span.resource.schema_url } : {}),
          scopeSpans: [],
        },
        scopes: new Map(),
      };
      resources.set(resourceKey, group);
    }

    const scope = span.scope ?? { name: '', version: '', attributes: {}, dropped_attributes_count: 0 };
    const scopeKey = JSON.stringify([scope.name, scope.version, scope.schema_url ?? '', sortedEntries(scope.attributes ?? {})]);
    let scopeSpans = group.scopes.get(scopeKey);
    if (!scopeSpans) {
      scopeSpans = {
        scope: {
          name: scope.name ?? '',
          version: scope.version ?? '',
          attributes: keyValues(scope.attributes ?? {}),
          droppedAttributesCount: scope.dropped_attributes_count ?? 0,
        },
        ...(scope.schema_url ? { schemaUrl: scope.schema_url } : {}),
        spans: [],
      };
      group.scopes.set(scopeKey, scopeSpans);
      group.entry.scopeSpans.push(scopeSpans);
    }

    scopeSpans.spans.push(otlpSpan(trace.traceId, span, Object.fromEntries(attributes)));
  }

  return { resourceSpans: [...resources.values()].map((group) => group.entry) };
}

function otlpSpan(traceId: string, span: Span, attributes: Record<string, unknown>): OtlpSpan {
  const start = nanos(span.startNs);
  const duration = nanos(span.durationNs);
  const end = duration > 0n ? start + duration : nanos(span.endNs);

  return {
    traceId,
    spanId: span.spanId,
    ...(span.parentSpanId ? { parentSpanId: span.parentSpanId } : {}),
    traceState: span.traceState ?? '',
    flags: span.traceFlags ?? 0,
    name: span.operation,
    kind: KINDS.indexOf(spanKindName(span.kind)),
    startTimeUnixNano: start.toString(),
    endTimeUnixNano: (end > start ? end : start).toString(),
    attributes: keyValues(attributes),
    droppedAttributesCount: span.droppedAttributesCount ?? 0,
    events: (span.events ?? []).map((event) => ({
      timeUnixNano: nanos(event.tsNs).toString(),
      name: event.name,
      attributes: keyValues(event.attributes ?? {}),
      droppedAttributesCount: event.droppedAttributesCount ?? 0,
    })),
    droppedEventsCount: span.droppedEventsCount ?? 0,
    links: (span.links ?? []).map((link) => ({
      traceId: link.trace_id,
      spanId: link.span_id,
      traceState: link.trace_state ?? '',
      flags: link.flags ?? 0,
      attributes: keyValues(link.attributes ?? {}),
      droppedAttributesCount: link.dropped_attributes_count ?? 0,
    })),
    droppedLinksCount: span.droppedLinksCount ?? 0,
    status: statusOf(span),
  };
}

/** OTLP's status codes: unset 0, ok 1, error 2. A span that timed out failed. */
function statusOf(span: Span): OtlpSpan['status'] {
  const status = String(span.status ?? '').toUpperCase();
  const code = status === 'OK' ? 1 : status === 'ERROR' || status === 'TIMED_OUT' ? 2 : 0;
  return span.statusMessage ? { code, message: span.statusMessage } : { code };
}

/** Epoch nanoseconds held as a number: exact to what the number keeps, as an integer. */
function nanos(value: unknown): bigint {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? BigInt(Math.round(number)) : 0n;
}

export function otlpAnyValue(value: unknown): OtlpAnyValue {
  if (value === null || value === undefined) return {};
  if (typeof value === 'string') return { stringValue: value };
  if (typeof value === 'boolean') return { boolValue: value };
  if (typeof value === 'bigint') return { intValue: value.toString() };
  if (typeof value === 'number') return Number.isInteger(value) ? { intValue: String(value) } : { doubleValue: value };
  if (Array.isArray(value)) return { arrayValue: { values: value.map(otlpAnyValue) } };
  if (typeof value === 'object') return { kvlistValue: { values: keyValues(value as Record<string, unknown>) } };
  return { stringValue: String(value) };
}

function keyValues(attributes: Record<string, unknown>): OtlpKeyValue[] {
  return sortedEntries(attributes).map(([key, value]) => ({ key, value: otlpAnyValue(value) }));
}

function sortedEntries(attributes: Record<string, unknown>): Array<[string, unknown]> {
  return Object.entries(attributes).sort(([left], [right]) => left.localeCompare(right));
}
