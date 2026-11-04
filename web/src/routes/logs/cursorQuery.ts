import * as webApi from '@/api/web';
import { type FieldExpression, parseFieldExpression } from '@/shell/query/fieldExpression';
import type { QueryResult } from '@/types/query';

import type { PinnedLogFilter } from './conditions/model';
import { logStatementFields, type ParsedLogFieldClause, readLogFieldPart } from './fieldQueryModel';

interface LogCursorQueryOptions {
  stream: string;
  statement: string;
  pinned: readonly PinnedLogFilter[];
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
  pinned,
  timeRange,
  pageSize,
  cursor,
}: LogCursorQueryOptions): Promise<LogCursorQueryResult> {
  const request = cursor
    ? { cursor, limit: pageSize }
    : firstPageRequest(stream, statement, pinned, timeRange, pageSize);
  const page = await webApi.logs(request);
  return { page, result: recordsToQueryResult(page.items) };
}

function firstPageRequest(
  stream: string,
  statement: string,
  pinned: readonly PinnedLogFilter[],
  timeRange: { start: number; end: number },
  pageSize: number,
): Parameters<typeof webApi.logs>[0] {
  return { ...logMatchRequest(stream, statement, pinned, timeRange), limit: pageSize };
}

/** What a structured log query matches, as the web endpoints take it. */
export interface LogMatchRequest {
  stream: string;
  from: number;
  to: number;
  filters: webApi.LogListFilter[];
  freeText: string[];
  /** The statement's alternatives, which the lists cannot say. */
  where?: webApi.LogCondition;
}

function listFilter({ field, op, value, quoted }: ParsedLogFieldClause): webApi.LogListFilter {
  return { field, op, value, quoted };
}

/**
 * A group of alternatives as the condition tree of a read; `null` where a part
 * of it is nothing the query can read, which `rejected` is told.
 */
function logCondition(expression: FieldExpression, rejected: string[]): webApi.LogCondition | null {
  if (expression.kind === 'part') {
    const read = readLogFieldPart(expression.text);
    if (read.kind === 'filter') return listFilter(read.filter);
    if (read.kind === 'text') return { text: read.value };
    rejected.push(expression.text);
    return null;
  }

  const items = expression.items.map((item) => logCondition(item, rejected));
  if (items.some((item) => item === null)) return null;
  const read = items as webApi.LogCondition[];
  return expression.kind === 'and' ? { and: read } : { or: read };
}

/**
 * The rows a field statement matches in a stream and a time range, with the
 * pinned filters applied on the stream's own fields (`pinnedLogFilters`) where
 * the statement does not already speak of them. The list asks for a page of
 * them and the histogram for their count, both with exactly this.
 *
 * What all has to hold goes as the lists of filters and words; alternatives,
 * `a OR b`, as a condition tree beside them.
 */
export function logMatchRequest(
  stream: string,
  statement: string,
  pinned: readonly PinnedLogFilter[],
  timeRange: { start: number; end: number },
): LogMatchRequest {
  const read = parseFieldExpression(statement);
  const parts = read === null ? [] : read.kind === 'and' ? read.items : [read];
  const filters: webApi.LogListFilter[] = [];
  const freeText: string[] = [];
  const alternatives: webApi.LogCondition[] = [];
  const rejected: string[] = [];

  for (const part of parts) {
    if (part.kind !== 'part') {
      const condition = logCondition(part, rejected);
      if (condition) alternatives.push(condition);
      continue;
    }
    const leaf = readLogFieldPart(part.text);
    if (leaf.kind === 'filter') filters.push(listFilter(leaf.filter));
    else if (leaf.kind === 'text') freeText.push(leaf.value);
    else rejected.push(part.text);
  }
  if (rejected.length > 0) {
    throw new Error(`Invalid Fields query: ${rejected.join(', ')}`);
  }

  // A field the statement speaks of anywhere is the reader's own choice.
  const named = new Set(logStatementFields(statement));
  for (const filter of pinned) {
    if (named.has(filter.field)) continue;
    named.add(filter.field);
    filters.push({ field: filter.field, op: filter.operator, value: filter.value, quoted: true });
  }
  const where = alternatives.length > 1 ? { and: alternatives } : alternatives[0];
  return {
    stream,
    from: timeRange.start,
    to: timeRange.end,
    filters,
    freeText: freeText,
    ...(where ? { where } : {}),
  };
}

/** Records as the result of a query: the columns are every field any of them carries. */
export function recordsToQueryResult(records: Record<string, unknown>[]): QueryResult {
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
    scannedRows: records.length,
    tookMs: 0,
  };
}
