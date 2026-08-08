import * as webApi from '@/api/web';
import type { GlobalFilter } from '@/stores/useFiltersStore';
import type { QueryResult } from '@/types/query';

import { parseLogFieldStatement } from './fieldQueryModel';

interface LogCursorQueryOptions {
  stream: string;
  statement: string;
  globalFilters: GlobalFilter[];
  timeRange: { start: number; end: number };
  pageSize: number;
  cursor?: string | undefined;
}

export interface LogCursorQueryResult {
  page: webApi.LogListResponse;
  result: QueryResult;
}

/**
 * Executes a structured log query through the shared cursor endpoint. A
 * continuation only needs the opaque cursor because it carries the frozen
 * time window and normalized filters.
 */
export async function runLogCursorQuery({
  stream,
  statement,
  globalFilters,
  timeRange,
  pageSize,
  cursor,
}: LogCursorQueryOptions): Promise<LogCursorQueryResult> {
  const request = cursor
    ? { cursor, limit: pageSize }
    : firstPageRequest(stream, statement, globalFilters, timeRange, pageSize);
  const page = await webApi.logs(request);
  return { page, result: recordsToQueryResult(page.items) };
}

function firstPageRequest(
  stream: string,
  statement: string,
  globalFilters: GlobalFilter[],
  timeRange: { start: number; end: number },
  pageSize: number,
): Parameters<typeof webApi.logs>[0] {
  const parsed = parseLogFieldStatement(statement);
  if (parsed.rejected.length > 0) {
    throw new Error(`Invalid Fields query: ${parsed.rejected.join(', ')}`);
  }
  const filters: webApi.LogListFilter[] = parsed.filters.map((filter) => ({
    field: filter.field,
    op: filter.op,
    value: filter.value,
    quoted: filter.quoted,
  }));
  for (const filter of globalFilters) {
    if (
      !filter.key ||
      !filter.value ||
      filters.some((item) => item.field === filter.key)
    ) {
      continue;
    }
    filters.push({
      field: filter.key,
      op: filter.operator === '!=' ? '!=' : '=',
      value: filter.value,
      quoted: true,
    });
  }
  return {
    stream,
    from: timeRange.start,
    to: timeRange.end,
    filters,
    free_text: parsed.freeText,
    limit: pageSize,
  };
}

function recordsToQueryResult(records: Record<string, unknown>[]): QueryResult {
  const columns = Array.from(
    records.reduce((names, record) => {
      Object.keys(record).forEach((name) => names.add(name));
      return names;
    }, new Set<string>()),
  );
  return {
    columns,
    rows: records.map((record) =>
      columns.map((column) => record[column] ?? null),
    ),
    scanned_rows: records.length,
    took_ms: 0,
  };
}
