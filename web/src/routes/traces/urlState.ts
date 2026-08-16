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

export function traceUrlQueryStateKey(state: TraceUrlQueryState): string {
  return JSON.stringify(state);
}

export function traceFieldQueryFromParams(params: URLSearchParams): string {
  const direct = params.get('q') ?? params.get('query') ?? '';
  if (direct.trim()) return direct.trim();
  const clauses: string[] = [];
  const traceId = params.get('trace_id') ?? params.get('traceId');
  const spanId = params.get('span_id') ?? params.get('spanId');
  const service = params.get('service') ?? params.get('service_name');
  const operation = params.get('operation_name') ?? params.get('operation');
  const route = params.get('route') ?? params.get('path');
  const status = params.get('status_code') ?? params.get('status');
  if (traceId) clauses.push(`trace_id = '${quoteTraceValue(traceId)}'`);
  if (spanId) clauses.push(`span_id = '${quoteTraceValue(spanId)}'`);
  if (service) clauses.push(`service_name = '${quoteTraceValue(service)}'`);
  if (operation) {
    clauses.push(`operation_name contains '${quoteTraceValue(operation)}'`);
  } else if (route) {
    clauses.push(`operation_name contains '${quoteTraceValue(route)}'`);
  }
  if (status) clauses.push(`status_code = '${quoteTraceValue(status)}'`);
  return clauses.join(' AND ');
}

export function quoteTraceValue(value: string): string {
  return value.replace(/\\/g, '\\\\').replace(/'/g, "\\'");
}
