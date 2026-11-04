import type { StreamField } from '@/api/streams';
import type { GlobalFilter } from '@/stores/useFiltersStore';

import { formatLogFieldQueryValue } from '../fieldQueryModel';
import { endingsOf, plainFieldName } from '../viewModel';

/**
 * What a condition from outside the editor is about. A link from another
 * signal asks for the logs of a trace, a service or a host; a filter pinned on
 * another page keeps to an environment. None of these is a field name: each
 * stream keeps it in a field of its own (`service.name` where OTLP wrote the
 * logs, `service` in a JSON line), and a query names that field.
 */
export type LogMeaning =
  | 'trace_id'
  | 'span_id'
  | 'service'
  | 'host'
  | 'pod'
  | 'route'
  | 'operation'
  | 'method'
  | 'status'
  | 'environment';

/**
 * The names a stream may give each meaning, the likeliest first. A name is
 * compared as a plain name (`service.name`, `service_name` and `serviceName`
 * are one), and a qualified one (`http.route`) is also found at the end of a
 * longer dotted name (`attributes.http.route`). A bare word is not: a
 * condition on the wrong field matches nothing, and `log.file.path` is no
 * route, `peer.service` the service that was called.
 */
const MEANING_NAMES: Readonly<Record<LogMeaning, readonly string[]>> = {
  trace_id: ['trace_id'],
  span_id: ['span_id'],
  service: ['service.name', 'service', 'svc'],
  host: ['host.name', 'host'],
  pod: ['k8s.pod.name', 'kubernetes.pod_name', 'kubernetes.pod.name', 'pod_name', 'pod'],
  route: ['http.route', 'route', 'url.path', 'path'],
  operation: ['operation_name', 'operation'],
  method: ['http.request.method', 'http.method', 'method'],
  status: ['http.response.status_code', 'http.status_code', 'status_code', 'status'],
  environment: ['deployment.environment.name', 'deployment.environment', 'environment', 'env'],
};

/** The meanings, in the order a link's conditions are written. */
export const LOG_MEANINGS = Object.keys(MEANING_NAMES) as LogMeaning[];

function qualified(name: string): boolean {
  return name.includes('.');
}

/** A field a condition can compare with: a structure takes no `=`. */
function comparable(field: StreamField): boolean {
  return field.dataType !== 'json';
}

/**
 * The stream's own field for a meaning: one named for it outright, or else one
 * whose dotted name ends in a qualified name of it. `null` when the stream
 * keeps none.
 */
export function logFieldFor(meaning: LogMeaning, fields: readonly StreamField[]): StreamField | null {
  const names = MEANING_NAMES[meaning];
  const candidates = fields.filter(comparable);

  for (const name of names) {
    const outright = candidates.find((field) => plainFieldName(field.name) === plainFieldName(name));
    if (outright) return outright;
  }

  for (const name of names.filter(qualified)) {
    const ending = candidates.find((field) => endingsOf(field.name).includes(plainFieldName(name)));
    if (ending) return ending;
  }

  return null;
}

/** What a name means, as another signal's label or a pinned filter's key gives it; `null` when nothing known. */
export function logMeaningOf(name: string): LogMeaning | null {
  const plain = plainFieldName(name);
  const endings = endingsOf(name);

  return (
    LOG_MEANINGS.find((meaning) => MEANING_NAMES[meaning].some((known) => plainFieldName(known) === plain)) ??
    LOG_MEANINGS.find((meaning) =>
      MEANING_NAMES[meaning].some((known) => qualified(known) && endings.includes(plainFieldName(known))),
    ) ??
    null
  );
}

/** What a link asks of the logs, by meaning: the field each is in depends on the stream. */
export type LogLinkRequest = Partial<Record<LogMeaning, string>>;

/** A condition a link asked for that the stream keeps no field for. */
export interface LogLinkGap {
  meaning: LogMeaning;
  value: string;
}

/** A name the field query takes as it is written: letters, digits, `_` and `.`. */
const WRITABLE_FIELD = /^[a-zA-Z_][\w.]*$/;

/** The same request, said the same way whatever order it was read in. */
export function logLinkKey(request: LogLinkRequest): string {
  return JSON.stringify(LOG_MEANINGS.flatMap((meaning) => (request[meaning] ? [[meaning, request[meaning]]] : [])));
}

/**
 * The field query a link asks for, in the stream's own field names, and what
 * it asked for that the stream keeps nowhere. Those are left out: a condition
 * on a field the stream does not have matches nothing, or is refused.
 */
export function logLinkStatement(
  request: LogLinkRequest,
  fields: readonly StreamField[],
): { statement: string; gaps: LogLinkGap[] } {
  const writable = fields.filter((field) => WRITABLE_FIELD.test(field.name));
  const clauses: string[] = [];
  const gaps: LogLinkGap[] = [];

  for (const meaning of LOG_MEANINGS) {
    const value = request[meaning];
    if (!value) continue;

    const field = logFieldFor(meaning, writable);
    if (field) clauses.push(`${field.name} = ${formatLogFieldQueryValue(typedValue(value, field))}`);
    else gaps.push({ meaning, value });
  }

  return { statement: clauses.join(' AND '), gaps };
}

/** A number for a field of numbers, written as one; anything else as text. */
function typedValue(value: string, field: StreamField): string | number {
  const numeric = field.dataType === 'int64' || field.dataType === 'float64';

  return numeric && /^-?\d+(?:\.\d+)?$/.test(value) ? Number(value) : value;
}

/** A pinned filter as it applies to one stream: on the stream's own field. */
export interface PinnedLogFilter {
  field: string;
  operator: '=' | '!=';
  value: string;
}

/**
 * The pinned filters as they apply to a stream. A filter is keyed by the label
 * it was picked from, which may be another signal's name for the thing
 * (`service_name` on a metric) and is kept in lower case. It applies on the
 * stream's field of that name, else on the stream's field for what the name
 * means. One the stream has no field for is not applied (`unmatched`).
 */
export function pinnedLogFilters(
  filters: readonly GlobalFilter[],
  fields: readonly StreamField[],
): { applied: PinnedLogFilter[]; unmatched: GlobalFilter[] } {
  const applied: PinnedLogFilter[] = [];
  const unmatched: GlobalFilter[] = [];

  for (const filter of filters) {
    if (!filter.key || !filter.value) continue;

    const field = namedField(filter.key, fields) ?? meantField(filter.key, fields);
    if (!field) {
      unmatched.push(filter);
      continue;
    }

    applied.push({ field: field.name, operator: filter.operator === '!=' ? '!=' : '=', value: filter.value });
  }

  return { applied, unmatched };
}

function namedField(name: string, fields: readonly StreamField[]): StreamField | null {
  const lower = name.toLowerCase();

  return (
    fields.find((field) => field.name === name) ??
    fields.find((field) => field.name.toLowerCase() === lower) ??
    null
  );
}

function meantField(name: string, fields: readonly StreamField[]): StreamField | null {
  const meaning = logMeaningOf(name);

  return meaning ? logFieldFor(meaning, fields) : null;
}
