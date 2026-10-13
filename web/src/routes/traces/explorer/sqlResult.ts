import type { QueryResult } from '@/types/query';

import type { DisplayTrace } from './types';

function traceColumnIndex(result: QueryResult): Record<string, number> {
  return result.columns.reduce<Record<string, number>>((acc, column, index) => {
    acc[column.toLowerCase()] = index;
    return acc;
  }, {});
}

function traceCell(row: unknown[], index: Record<string, number>, names: string[]): unknown {
  for (const name of names) {
    const hit = index[name.toLowerCase()];
    if (hit !== undefined) return row[hit];
  }
  return undefined;
}

function stringTraceCell(row: unknown[], index: Record<string, number>, names: string[], fallback = '-'): string {
  const value = traceCell(row, index, names);
  return value === null || value === undefined || value === '' ? fallback : String(value);
}

function numberTraceCell(row: unknown[], index: Record<string, number>, names: string[], fallback = 0): number {
  const value = traceCell(row, index, names);
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string' && value.trim()) {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return fallback;
}

/**
 * The rows of a SQL statement, read as the traces the explorer lists. A statement names its
 * columns as it likes, so each is looked for under the names it usually goes by.
 */
export function traceSqlResultToDisplayTraces(result: QueryResult): DisplayTrace[] {
  const index = traceColumnIndex(result);
  return result.rows.map((row, rowIndex) => {
    const startNs = numberTraceCell(row, index, ['start_ns', 'start_time_unix_nano'], 0);
    const endNs = numberTraceCell(row, index, ['end_ns', 'end_time_unix_nano'], 0);
    const durationMs = numberTraceCell(
      row,
      index,
      ['duration_ms'],
      numberTraceCell(row, index, ['duration_us'], endNs > startNs ? (endNs - startNs) / 1000 : 0) / 1000,
    );
    const status = stringTraceCell(row, index, ['status_code', 'status'], 'OK').toUpperCase();
    return {
      id: stringTraceCell(row, index, ['trace_id', 'id'], `sql-row-${rowIndex + 1}`),
      op: stringTraceCell(row, index, ['operation', 'op', 'operation_name'], '-'),
      service: stringTraceCell(row, index, ['service', 'service_name'], '-'),
      startNs,
      durationMs,
      spans: Math.max(1, Math.round(numberTraceCell(row, index, ['span_count', 'spans', 'count'], 1))),
      errors: Math.max(0, Math.round(numberTraceCell(row, index, ['error_count', 'errors'], status === 'ERROR' ? 1 : 0))),
    };
  });
}
