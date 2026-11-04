import type { Span } from '@/api/web';

/** Where a span keeps the HTTP status it answered with, the current name first. */
export const HTTP_STATUS_KEYS = ['http.response.status_code', 'http.status_code'] as const;

/** The HTTP status a span answered with; `null` for a span that kept none. */
export function httpStatusOf(span: Pick<Span, 'attributes'>): number | null {
  for (const key of HTTP_STATUS_KEYS) {
    const status = statusNumber(span.attributes?.[key]);
    if (status !== null) return status;
  }

  return null;
}

/** An HTTP status as a number: stored as one, or as its digits. */
export function statusNumber(value: unknown): number | null {
  const number = typeof value === 'number' ? value : typeof value === 'string' ? Number(value.trim()) : NaN;
  return Number.isInteger(number) && number >= 100 && number < 600 ? number : null;
}

/** What an HTTP status says, by its class: answered, sent elsewhere, the caller's fault, the server's. */
export type HttpStatusTone = 'ok' | 'redirect' | 'client' | 'server';

export function httpStatusTone(status: number): HttpStatusTone {
  if (status >= 500) return 'server';
  if (status >= 400) return 'client';
  if (status >= 300) return 'redirect';
  return 'ok';
}

/** The ink each class is written in: a status is a word in its colour, with nothing behind it. */
export const HTTP_STATUS_TEXT: Record<HttpStatusTone, string> = {
  ok: 'text-[var(--green-ink)]',
  redirect: 'text-[var(--yellow-ink)]',
  client: 'text-[var(--orange-ink)]',
  server: 'text-[var(--red-ink)]',
};

/**
 * How a request ended, as its row says it: failed (an error, or the server's
 * 5xx), refused (a 4xx: the caller's fault, and still no answer) or answered.
 */
export type RequestOutcome = 'failed' | 'refused' | 'ok';

export function requestOutcome(failed: boolean, httpStatus: number | null | undefined): RequestOutcome {
  if (failed || (httpStatus != null && httpStatus >= 500)) return 'failed';
  if (httpStatus != null && httpStatus >= 400) return 'refused';
  return 'ok';
}

/** The mark of an outcome: a dot beside a title, or the edge down a row. */
export const OUTCOME_FILL: Record<RequestOutcome, string> = {
  failed: 'bg-red',
  refused: 'bg-[var(--orange)]',
  ok: 'bg-[var(--green)]',
};

/** The edge a row carries down its left side, in the tones' fills: an answered row's is quieter than one that went wrong. */
export const OUTCOME_EDGE: Record<RequestOutcome, string> = {
  failed: 'bg-[var(--red-fill)]',
  refused: 'bg-[var(--orange-fill)]',
  ok: 'bg-[var(--green-fill)] opacity-60',
};

/** The word of an outcome in its colour. */
export const OUTCOME_TEXT: Record<RequestOutcome, string> = {
  failed: 'text-[var(--red-ink)]',
  refused: 'text-[var(--orange-ink)]',
  ok: 'text-[var(--green-ink)]',
};

export type SpanKindName = 'unspecified' | 'internal' | 'server' | 'client' | 'producer' | 'consumer';

const KIND_NAMES: readonly SpanKindName[] = ['unspecified', 'internal', 'server', 'client', 'producer', 'consumer'];

/** A kind as OTLP numbers it, or by its name, kept either way. */
export function spanKindName(kind: unknown): SpanKindName {
  if (typeof kind === 'number') return KIND_NAMES[kind] ?? 'unspecified';
  if (typeof kind !== 'string') return 'unspecified';

  const digits = Number(kind);
  if (kind.trim() !== '' && Number.isInteger(digits)) return KIND_NAMES[digits] ?? 'unspecified';

  const name = kind.trim().toLowerCase().replace(/^span_kind_/, '');
  return (KIND_NAMES as readonly string[]).includes(name) ? (name as SpanKindName) : 'unspecified';
}

/** An exception a span recorded, as OpenTelemetry names its parts. */
export interface SpanException {
  type: string;
  message: string;
  stacktrace: string;
  tsNs: number;
}

export function spanExceptions(span: Pick<Span, 'events'>): SpanException[] {
  return (span.events ?? [])
    .filter((event) => event.name === 'exception')
    .map((event) => ({
      type: text(event.attributes?.['exception.type']),
      message: text(event.attributes?.['exception.message']),
      stacktrace: text(event.attributes?.['exception.stacktrace']),
      tsNs: event.tsNs,
    }));
}

function text(value: unknown): string {
  if (value === undefined || value === null) return '';
  return typeof value === 'string' ? value : JSON.stringify(value);
}

export type AttributeEntry = readonly [key: string, value: unknown];

/**
 * A span's own tags and those of the resource that emitted it, each said
 * once. The store flattens a resource's attributes into the span's row, so
 * a tag the resource holds with the same value is the resource's.
 */
export function splitAttributes(span: Pick<Span, 'attributes' | 'resource'>): {
  attributes: AttributeEntry[];
  resource: AttributeEntry[];
} {
  const resource = entries(span.resource?.attributes);
  const resourceValues = new Map(resource.map(([key, value]) => [key, JSON.stringify(value)]));

  const attributes = entries(span.attributes).filter(
    ([key, value]) => !key.startsWith('molesignal.trace.') && resourceValues.get(key) !== JSON.stringify(value),
  );

  return { attributes, resource };
}

function entries(input: unknown): AttributeEntry[] {
  let value = input;

  if (typeof value === 'string') {
    try {
      value = JSON.parse(value);
    } catch {
      return [['value', value]];
    }
  }

  if (!value || typeof value !== 'object' || Array.isArray(value)) return [];
  return Object.entries(value as Record<string, unknown>).sort(([left], [right]) => left.localeCompare(right));
}

/** The part of a span a tag describes, by the namespace of its name. */
export type AttributeGroup =
  | 'http'
  | 'database'
  | 'rpc'
  | 'messaging'
  | 'error'
  | 'code'
  | 'service'
  | 'deployment'
  | 'host'
  | 'process'
  | 'container'
  | 'telemetry'
  | 'other';

const GROUP_PREFIXES: ReadonlyArray<readonly [AttributeGroup, readonly string[]]> = [
  ['http', ['http.', 'url.', 'user_agent.', 'server.', 'client.', 'network.', 'net.']],
  ['database', ['db.']],
  ['rpc', ['rpc.', 'grpc.']],
  ['messaging', ['messaging.']],
  ['error', ['exception.', 'error.']],
  ['code', ['code.', 'thread.']],
  ['service', ['service.']],
  ['deployment', ['deployment.']],
  ['host', ['host.', 'os.', 'node.']],
  ['process', ['process.']],
  ['container', ['container.', 'k8s.', 'cloud.']],
  ['telemetry', ['telemetry.', 'otel.']],
];

export function attributeGroup(key: string): AttributeGroup {
  for (const [group, prefixes] of GROUP_PREFIXES) {
    if (prefixes.some((prefix) => key.startsWith(prefix)) || key === 'error') return group;
  }

  return 'other';
}

/** Tags in the order of their groups, as listed above, and by name within one. */
export function groupAttributes(list: readonly AttributeEntry[]): Array<{ group: AttributeGroup; entries: AttributeEntry[] }> {
  const groups = new Map<AttributeGroup, AttributeEntry[]>();

  for (const entry of list) {
    const group = attributeGroup(entry[0]);
    groups.set(group, [...(groups.get(group) ?? []), entry]);
  }

  const order = [...GROUP_PREFIXES.map(([group]) => group), 'other' as const];
  return order.filter((group) => groups.has(group)).map((group) => ({ group, entries: groups.get(group)! }));
}

/** What a tag's value reads as. */
export function attributeText(value: unknown): string {
  return typeof value === 'string' ? value : (JSON.stringify(value) ?? String(value));
}
