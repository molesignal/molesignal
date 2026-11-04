import type { SpanListItem, SpanListSort } from '@/api/traceExplorer';
import type { TraceWindow } from '@/api/web';
import type { QueryResult } from '@/types/query';

export const SPAN_LIST_SORTS: readonly SpanListSort[] = ['latest', 'earliest', 'duration_desc', 'duration_asc'];

/** The order the span list is in, as the address keeps it apart from the trace list's. */
export function parseSpanListSort(value: string | null): SpanListSort {
  return SPAN_LIST_SORTS.includes(value as SpanListSort) ? (value as SpanListSort) : 'latest';
}

export function writeSpanListSort(current: URLSearchParams, sort: SpanListSort): URLSearchParams {
  const next = new URLSearchParams(current);
  if (sort === 'latest') next.delete('spanSort');
  else next.set('spanSort', sort);
  return next;
}

/** A column of the span list a click sorts by: when it began, or how long it took. */
export type SpanSortColumn = 'start' | 'duration';

export function nextSpanSort(column: SpanSortColumn, current: SpanListSort): SpanListSort {
  if (column === 'start') return current === 'latest' ? 'earliest' : 'latest';
  return current === 'duration_desc' ? 'duration_asc' : 'duration_desc';
}

export function spanSortDirection(column: SpanSortColumn, current: SpanListSort): 'descending' | 'ascending' | null {
  if (column === 'start' && (current === 'latest' || current === 'earliest')) return current === 'latest' ? 'descending' : 'ascending';
  if (column === 'duration' && (current === 'duration_desc' || current === 'duration_asc')) {
    return current === 'duration_desc' ? 'descending' : 'ascending';
  }
  return null;
}

/** An hour either side of a span: where its trace's spans are looked for. */
const AROUND_MICROS = 60 * 60 * 1_000_000;

export function spanTraceWindow(startNs: number): TraceWindow | undefined {
  if (!Number.isFinite(startNs) || startNs <= 0) return undefined;
  const start = Math.floor(startNs / 1_000);
  return { from: start - AROUND_MICROS, to: start + AROUND_MICROS };
}

function columnIndex(result: QueryResult): Map<string, number> {
  return new Map(result.columns.map((column, index) => [column.toLowerCase(), index]));
}

/**
 * The rows of a statement read as spans, where they say which span they are:
 * a trace id and a span id. A statement names its columns as it likes, so
 * each is looked for under the names it usually goes by.
 */
export function spanSqlResultToItems(result: QueryResult): SpanListItem[] {
  const index = columnIndex(result);
  const cell = (row: unknown[], names: readonly string[]) => {
    for (const name of names) {
      const at = index.get(name);
      if (at !== undefined && row[at] !== null && row[at] !== undefined) return row[at];
    }
    return undefined;
  };
  const text = (row: unknown[], names: readonly string[]) => {
    const value = cell(row, names);
    return value === undefined ? '' : String(value);
  };
  const number = (row: unknown[], names: readonly string[]) => {
    const value = Number(cell(row, names));
    return Number.isFinite(value) ? value : 0;
  };

  return result.rows.flatMap((row): SpanListItem[] => {
    const traceId = text(row, ['trace_id']);
    const spanId = text(row, ['span_id']);
    if (!traceId || !spanId) return [];

    const startNs = number(row, ['start_time_unix_nano', 'start_ns']) || number(row, ['_timestamp']) * 1_000;
    const endNs = number(row, ['end_time_unix_nano', 'end_ns']);
    const parent = text(row, ['parent_span_id']);
    const kind = number(row, ['kind']);

    return [
      {
        traceId: traceId,
        spanId: spanId,
        ...(parent ? { parentSpanId: parent } : {}),
        service: text(row, ['service.name', 'service']),
        operation: text(row, ['name', 'operation']),
        startNs: startNs,
        durationNs: number(row, ['duration_ns']) || (endNs > startNs ? endNs - startNs : 0),
        status: text(row, ['status_code', 'status']).toUpperCase(),
        ...(kind > 0 ? { kind } : {}),
      },
    ];
  });
}
