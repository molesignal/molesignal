import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as rumApi from '@/api/rum';
import type { RumRow } from '@/api/rum';
import type { CursorPage } from '@/pagination/cursor';
import { DEFAULT_EXPLORER_PAGE_SIZE, EXPLORER_PAGE_SIZE_OPTIONS } from '@/pagination/explorerPageSize';
import { useCursorPagination } from '@/pagination/useCursorPagination';

import type { ParsedRumStatement } from './fieldQuery';
import type { RumTab } from './model';

export interface RumListQuery {
  orgId: string;
  tab: RumTab;
  /** The range the rows are asked from, as instants. */
  range: { from: string; to: string };
  /** The statement that is applied: it names the query in the address and in the cache. */
  statement: string;
  parsed: ParsedRumStatement;
  /** The columns the table shows; the server adds the one that names a row. */
  columns: readonly string[];
  /** Nothing is asked until the fields of the type are known. */
  enabled: boolean;
}

export interface RumPaginationModel {
  pageSize: number;
  pageSizeOptions: number[];
  hasPrevious: boolean;
  hasNext: boolean;
  pending: boolean;
  onPrevious: () => void;
  onNext: () => void;
  onPageSizeChange: (pageSize: number) => void;
}

/**
 * The rows the query matches, a page at a time. Another type, query, range or set of columns
 * starts from the first page.
 */
export function useRumList({ orgId, tab, range, statement, parsed, columns, enabled }: RumListQuery) {
  const contextKey = React.useMemo(
    () => JSON.stringify([orgId, tab, range.from, range.to, statement, columns]),
    [columns, orgId, range.from, range.to, statement, tab],
  );
  const pages = useCursorPagination({ contextKey, defaultPageSize: DEFAULT_EXPLORER_PAGE_SIZE });

  const query = useQuery<CursorPage<RumRow>>({
    queryKey: ['rum', 'search', contextKey, pages.cursor, pages.pageSize],
    enabled,
    queryFn: () =>
      rumApi.search(
        pages.cursor
          ? { type: tab, limit: pages.pageSize, cursor: pages.cursor }
          : {
              type: tab,
              from: Date.parse(range.from) * 1_000,
              to: Date.parse(range.to) * 1_000,
              limit: pages.pageSize,
              columns: [...columns],
              ...(parsed.filters.length > 0 ? { filters: parsed.filters } : {}),
              ...(parsed.freeText.length > 0 ? { freeText: parsed.freeText } : {}),
            },
      ),
  });

  const rows = React.useMemo(() => query.data?.items ?? [], [query.data?.items]);

  const pagination = React.useMemo<RumPaginationModel>(
    () => ({
      pageSize: pages.pageSize,
      pageSizeOptions: EXPLORER_PAGE_SIZE_OPTIONS,
      hasPrevious: Boolean(query.data?.previousCursor),
      hasNext: Boolean(query.data?.nextCursor),
      pending: query.isFetching,
      onPrevious: () => pages.goPrevious(query.data),
      onNext: () => pages.goNext(query.data),
      onPageSizeChange: pages.setPageSize,
    }),
    [pages, query.data, query.isFetching],
  );

  return { query, rows, pagination, resetCursor: pages.reset };
}
