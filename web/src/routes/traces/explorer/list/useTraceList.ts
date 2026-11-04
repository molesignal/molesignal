import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as queryApi from '@/api/query';
import * as webApi from '@/api/web';
import type { CursorPage } from '@/pagination/cursor';
import { explorerPageSizeIn, explorerPageSizesFor } from '@/pagination/explorerPageSize';
import { useCursorPagination } from '@/pagination/useCursorPagination';

import { type ParsedTraceStatement, TRACE_RESULT_LIMIT, type TraceQueryMode } from '../../fieldQueryModel';
import { DEFAULT_TRACE_PAGE_SIZE, type TracePaginationModel } from '../../Pagination';
import { traceSqlResultToDisplayTraces } from '../sqlResult';
import type { DisplayTrace } from '../types';

export interface TraceListQuery {
  orgId: string;
  range: { from: string; to: string };
  queryMode: TraceQueryMode;
  queryText: string;
  sqlText: string;
  parsedQuery: ParsedTraceStatement;
  stream: string;
  sort: webApi.TraceListSort;
  /** Fields of each trace's root span to list as columns. */
  rootColumns: readonly string[];
}

/** A trace as the list endpoint answers it, as the explorer lists it. */
export function displayTrace(item: webApi.TraceListItem): DisplayTrace {
  return {
    id: item.traceId,
    op: item.operation,
    service: item.service,
    startNs: item.startNs,
    durationMs: item.durationMs,
    spans: item.spanCount,
    errors: item.errorCount,
    ...(item.status ? { status: item.status } : {}),
    ...(item.httpStatusCode ? { httpStatus: item.httpStatusCode } : {}),
    ...(item.fields ? { root: item.fields } : {}),
  };
}

/**
 * The traces the query matches, a page at a time from the list endpoint, or
 * all a statement returned, paged here. Another query, range or order starts
 * from the first page.
 */
export function useTraceList({ orgId, range, queryMode, queryText, sqlText, parsedQuery, stream, sort, rootColumns }: TraceListQuery) {
  const contextKey = React.useMemo(
    () => JSON.stringify([orgId, range.from, range.to, queryMode, queryText, sort]),
    [orgId, queryMode, queryText, range.from, range.to, sort],
  );
  const pages = useCursorPagination({ contextKey, defaultPageSize: DEFAULT_TRACE_PAGE_SIZE });
  const [statementPage, setStatementPage] = React.useState(1);

  const query = useQuery<CursorPage<DisplayTrace>>({
    queryKey: ['web', 'traces', range.from, range.to, queryMode, queryText, sqlText, sort, stream, orgId, pages.cursor, pages.pageSize, rootColumns.join(',')],
    enabled: queryMode === 'fields' || Boolean(orgId && stream && sqlText.trim()),
    queryFn: async () => {
      if (queryMode === 'sql') {
        const statement = sqlText.trim();
        if (!statement || !stream) return { items: [], nextCursor: null, previousCursor: null, hasMore: false };

        const result = await queryApi.readQuery({
          orgId: orgId,
          language: 'sql',
          statement,
          timeRange: { start: Date.parse(range.from) * 1_000, end: Date.parse(range.to) * 1_000 },
          stream: { name: stream, streamType: 'traces' },
          limit: TRACE_RESULT_LIMIT,
        });
        return { items: traceSqlResultToDisplayTraces(result), nextCursor: null, previousCursor: null, hasMore: false };
      }

      const fields = rootColumns.length > 0 ? { fields: [...rootColumns] } : {};
      const response = await webApi.traces(
        pages.cursor
          ? { limit: pages.pageSize, cursor: pages.cursor, ...fields }
          : {
              from: Date.parse(range.from) * 1_000,
              to: Date.parse(range.to) * 1_000,
              limit: pages.pageSize,
              ...(parsedQuery.q ? { q: parsedQuery.q } : {}),
              ...(parsedQuery.filters.length > 0 ? { filters: parsedQuery.filters } : {}),
              ...(parsedQuery.where ? { where: parsedQuery.where } : {}),
              sort,
              ...fields,
            },
      );
      return { ...response, items: (response.items ?? []).map(displayTrace) };
    },
  });

  const traces = React.useMemo(() => query.data?.items ?? [], [query.data?.items]);

  // What a statement returned is all in hand and paged here: no size is offered beyond the one that shows it all.
  const statementPageSizes = React.useMemo(() => explorerPageSizesFor(traces.length), [traces.length]);
  const statementPageSize = explorerPageSizeIn(statementPageSizes, pages.pageSize);
  const pageCount = queryMode === 'sql' ? Math.max(1, Math.ceil(traces.length / statementPageSize)) : 1;
  const activePage = Math.min(statementPage, pageCount);
  const pageStart = (activePage - 1) * statementPageSize;
  const pagedTraces = React.useMemo(
    () => (queryMode === 'fields' ? traces : traces.slice(pageStart, pageStart + statementPageSize)),
    [pageStart, queryMode, statementPageSize, traces],
  );

  React.useEffect(() => setStatementPage(1), [queryMode, queryText, range.from, range.to, sqlText, sort]);

  const pagination = React.useMemo<TracePaginationModel>(
    () =>
      queryMode === 'fields'
        ? {
            kind: 'cursor',
            pageSize: pages.pageSize,
            hasPrevious: Boolean(query.data?.previousCursor),
            hasNext: Boolean(query.data?.nextCursor),
            pending: query.isFetching,
            onPrevious: () => pages.goPrevious(query.data),
            onNext: () => pages.goNext(query.data),
            onPageSizeChange: (size) => {
              pages.setPageSize(size);
              setStatementPage(1);
            },
          }
        : {
            kind: 'offset',
            page: activePage,
            pageCount,
            pageSize: statementPageSize,
            pageSizes: statementPageSizes,
            onPageChange: (page) => setStatementPage(Math.min(Math.max(1, page), pageCount)),
            onPageSizeChange: (size) => {
              pages.setPageSize(size);
              setStatementPage(1);
            },
          },
    [activePage, pageCount, pages, query.data, query.isFetching, queryMode, statementPageSize, statementPageSizes],
  );

  return { query, traces, pagedTraces, pagination, cursor: pages.cursor, resetCursor: pages.reset };
}
