import type { TransactionIdentity } from '@/api/apm';
import type { TraceFilter } from '@/api/web';
import { toApiError } from '@/lib/http';
import { quoteTraceValue } from '@/routes/traces/urlState';

import { OTHER_DIMENSION } from './scope';

/**
 * The spans of an endpoint, as filters of the trace store. APM names an
 * endpoint from its entry spans (`GET /orders/{id}`, `Svc/Method`,
 * `publish orders`); the same fields find them again. Where the projector
 * reads one of two field names (the current convention and the one before
 * it), each is a candidate in that order: the first that finds spans holds.
 * An endpoint folded into the overflow row has none.
 */
export function endpointFilterCandidates(service: string, transaction: TransactionIdentity): TraceFilter[][] {
  if (transaction.name === OTHER_DIMENSION || transaction.kind === 'other') return [];

  const base: TraceFilter = { field: 'service.name', op: '=', value: service };
  const equal = (field: string, value: string): TraceFilter => ({
    field,
    op: '=',
    value,
  });

  if (transaction.kind === 'http') {
    const [method, route] = splitOnce(transaction.name, ' ');
    return ['http.request.method', 'http.method'].map((methodField) => [base, equal(methodField, method), ...(route ? [equal('http.route', route)] : [])]);
  }

  if (transaction.kind === 'rpc') {
    const cut = transaction.name.lastIndexOf('/');
    const rpcService = cut > 0 ? transaction.name.slice(0, cut) : transaction.name;
    const rpcMethod = cut > 0 ? transaction.name.slice(cut + 1) : '';
    return [[base, equal('rpc.service', rpcService), ...(rpcMethod ? [equal('rpc.method', rpcMethod)] : [])]];
  }

  if (transaction.kind === 'messaging') {
    const [operation, destination] = splitOnce(transaction.name, ' ');
    return [
      ['messaging.operation.name', 'messaging.destination.template'],
      ['messaging.operation', 'messaging.destination.name'],
    ].map(([operationField, destinationField]) => [base, equal(operationField!, operation), ...(destination ? [equal(destinationField!, destination)] : [])]);
  }

  return [[base, equal('name', transaction.name)]];
}

/**
 * Whether the trace store refused a candidate for naming a field its spans
 * do not have. Such a spelling finds nothing, and the next one may.
 */
export function isUnknownTraceField(error: unknown): boolean {
  const { status, message } = toApiError(error);
  return status === 400 && message.includes('unsupported trace filter field');
}

/** Filters as the field query of the Traces page writes them. */
export function traceFieldQuery(filters: readonly TraceFilter[]): string {
  return filters.map((filter) => `${filter.field} ${filter.op} '${quoteTraceValue(filter.value)}'`).join(' AND ');
}

/** The Traces page, showing the spans the filters find in the time given. */
export function tracesHref(filters: readonly TraceFilter[], range: { from: number; to: number }, tab: 'traces' | 'spans' = 'traces'): string {
  const params = new URLSearchParams({
    q: traceFieldQuery(filters),
    from: new Date(range.from / 1_000).toISOString(),
    to: new Date(range.to / 1_000).toISOString(),
  });
  if (tab === 'spans') params.set('tab', 'spans');
  return `/traces?${params}`;
}

function splitOnce(value: string, separator: string): [string, string] {
  const cut = value.indexOf(separator);
  return cut < 0 ? [value, ''] : [value.slice(0, cut), value.slice(cut + separator.length)];
}
