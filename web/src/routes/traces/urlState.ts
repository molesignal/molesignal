import type { TraceQueryMode } from './fieldQueryModel';

export interface TraceUrlQueryState {
  mode: TraceQueryMode;
  fields: string;
  sql: string;
}

/** Parse the Explore query contract, giving explicit `sql=` precedence. */
export function traceUrlQueryStateFromParams(
  params: URLSearchParams,
): TraceUrlQueryState {
  const sql = params.get('sql')?.trim() ?? '';
  const fields = traceFieldQueryFromParams(params);
  return {
    mode: sql ? 'sql' : 'fields',
    fields,
    sql,
  };
}

/** The parameters a link may carry a query in; the query applied on the page replaces them all. */
export const TRACE_QUERY_PARAMS = [
  'q',
  'query',
  'sql',
  'traceId',
  'spanId',
  'service',
  'serviceName',
  'operation',
  'operationName',
  'path',
  'route',
  'status',
  'statusCode',
];

/**
 * The address of the query applied on the page: its statement in `q` or in
 * `sql` by its language, in place of whatever the address asked for before.
 * No statement is no query.
 */
export function writeTraceQueryParams(
  current: URLSearchParams,
  mode: TraceQueryMode,
  statement: string,
): URLSearchParams {
  const next = new URLSearchParams(current);
  for (const key of TRACE_QUERY_PARAMS) next.delete(key);

  const trimmed = statement.trim();
  if (trimmed) next.set(mode === 'sql' ? 'sql' : 'q', trimmed);

  return next;
}

export function traceUrlQueryStateKey(state: TraceUrlQueryState): string {
  return JSON.stringify(state);
}

export function traceFieldQueryFromParams(params: URLSearchParams): string {
  const direct = params.get('q') ?? params.get('query') ?? '';
  if (direct.trim()) return direct.trim();
  const clauses: string[] = [];
  const traceId = params.get('traceId');
  const spanId = params.get('spanId');
  const service = params.get('service') ?? params.get('serviceName');
  const operation = params.get('operationName') ?? params.get('operation');
  const route = params.get('route') ?? params.get('path');
  const status = params.get('statusCode') ?? params.get('status');
  if (traceId) clauses.push(`trace_id = '${quoteTraceValue(traceId)}'`);
  if (spanId) clauses.push(`span_id = '${quoteTraceValue(spanId)}'`);
  if (service) clauses.push(`service.name = '${quoteTraceValue(service)}'`);
  if (operation) {
    clauses.push(`name contains '${quoteTraceValue(operation)}'`);
  } else if (route) {
    clauses.push(`name contains '${quoteTraceValue(route)}'`);
  }
  if (status) clauses.push(`status_code = '${quoteTraceValue(status)}'`);
  return clauses.join(' AND ');
}

export function quoteTraceValue(value: string): string {
  return value.replace(/\\/g, '\\\\').replace(/'/g, "\\'");
}
