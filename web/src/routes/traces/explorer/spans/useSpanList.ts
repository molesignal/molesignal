import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as queryApi from '@/api/query';
import { type SpanListItem, type SpanListSort, traceSpans } from '@/api/traceExplorer';
import type { CursorPage } from '@/pagination/cursor';
import { useCursorPagination } from '@/pagination/useCursorPagination';

import { spanSqlResultToItems } from './model';
import type { ParsedTraceStatement, TraceQueryMode } from '../../fieldQueryModel';
import { DEFAULT_TRACE_PAGE_SIZE, type TracePaginationModel } from '../../Pagination';

/** The most rows a statement's spans are read from at once. */
const STATEMENT_LIMIT = 5_000;

export interface SpanListQuery {
  orgId: string;
  range: { from: string; to: string };
  queryMode: TraceQueryMode;
  queryText: string;
  sqlText: string;
  parsedQuery: ParsedTraceStatement;
  stream: string;
  sort: SpanListSort;
  /** The span list is on screen: it is only read while it is. */
  enabled: boolean;
}

/**
 * The spans the query matches, a page at a time, newest first unless the
 * reader sorts otherwise. A statement the reader wrote is run as it is, and
 * its rows that name a span are listed.
 */
export function useSpanList(query: SpanListQuery) {
  const { orgId, range, queryMode, queryText, sqlText, parsedQuery, stream, sort, enabled } = query;
  const contextKey = JSON.stringify([orgId, range.from, range.to, queryMode, queryText, sqlText, sort]);
  const pages = useCursorPagination({ contextKey, defaultPageSize: DEFAULT_TRACE_PAGE_SIZE });

  const list = useQuery<CursorPage<SpanListItem>>({
    queryKey: ['web', 'trace-spans', contextKey, stream, pages.cursor, pages.pageSize],
    enabled: enabled && (queryMode === 'fields' || Boolean(orgId && stream && sqlText.trim())),
    queryFn: async () => {
      if (queryMode === 'sql') {
        const result = await queryApi.readQuery({
          orgId: orgId,
          language: 'sql',
          statement: sqlText.trim(),
          timeRange: { start: Date.parse(range.from) * 1_000, end: Date.parse(range.to) * 1_000 },
          stream: { name: stream, streamType: 'traces' },
          limit: STATEMENT_LIMIT,
        });
        return { items: spanSqlResultToItems(result), nextCursor: null, previousCursor: null, hasMore: false };
      }

      return traceSpans(
        pages.cursor
          ? { limit: pages.pageSize, cursor: pages.cursor }
          : {
              from: Date.parse(range.from) * 1_000,
              to: Date.parse(range.to) * 1_000,
              limit: pages.pageSize,
              ...(parsedQuery.q ? { q: parsedQuery.q } : {}),
              ...(parsedQuery.filters.length > 0 ? { filters: parsedQuery.filters } : {}),
              ...(parsedQuery.where ? { where: parsedQuery.where } : {}),
              sort,
            },
      );
    },
  });

  const spans = React.useMemo(() => list.data?.items ?? [], [list.data?.items]);

  const pagination = React.useMemo<TracePaginationModel>(
    () => ({
      kind: 'cursor',
      pageSize: pages.pageSize,
      hasPrevious: Boolean(list.data?.previousCursor),
      hasNext: Boolean(list.data?.nextCursor),
      pending: list.isFetching,
      onPrevious: () => pages.goPrevious(list.data),
      onNext: () => pages.goNext(list.data),
      onPageSizeChange: pages.setPageSize,
    }),
    [list.data, list.isFetching, pages],
  );

  return { list, spans, pagination, resetCursor: pages.reset };
}
