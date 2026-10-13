import { useInfiniteQuery } from '@tanstack/react-query';
import * as React from 'react';

import type { GlobalFilter } from '@/stores/useFiltersStore';
import type { QueryResult } from '@/types/query';

import { recordsToQueryResult, runLogCursorQuery } from '../cursorQuery';

/** How many logs are read at a time, at the least: enough for the kinds of message to show, few enough to wait for. */
export const PATTERN_SAMPLE_STEP = 500;

/** The field query the patterns are of, as it was run. */
export interface PatternSampleQuery {
  orgId: string;
  stream: string;
  statement: string;
  globalFilters: GlobalFilter[];
  timeRange: { start: number; end: number };
  /** When the query was run: running it again reads again. */
  ranAt: number;
  /**
   * How many logs are read at a time, and asked for in one answer: `PATTERN_SAMPLE_STEP`,
   * or a page of the list where the reader has set that larger.
   */
  step: number;
}

export interface PatternSample {
  /** The logs read so far as one result, the newest first; none before the first answer. */
  result: QueryResult | undefined;
  /** More logs are being read. */
  reading: boolean;
  /** Older logs match and have not been read. */
  more: boolean;
  /** The last answer asked for did not come. */
  failed: boolean;
  readMore: () => void;
  retry: () => void;
}

/**
 * The logs the patterns are read from.
 *
 * A page of the list starts at a hundred logs, and that is few to say what
 * kinds of message there are. So the patterns ask the list's own endpoint with
 * the list's own query for the latest few hundred logs that match; more,
 * further back, when asked. They are the logs the list itself would page
 * through. A step is asked for in one answer; a backend that gives fewer in an
 * answer is asked again until the step is read.
 *
 * Without a query (`null`) nothing is read.
 */
export function useLogPatternSample(query: PatternSampleQuery | null): PatternSample {
  const key = React.useMemo(
    () =>
      query &&
      JSON.stringify([
        query.orgId,
        query.stream,
        query.statement,
        query.globalFilters,
        query.timeRange.start,
        query.timeRange.end,
        query.ranAt,
        query.step,
      ]),
    [query],
  );
  // How far the reader asked to read, which is of one query: another starts over.
  const step = query?.step ?? PATTERN_SAMPLE_STEP;
  const [asked, setAsked] = React.useState<{ key: string | null; logs: number }>({ key, logs: step });
  const wanted = asked.key === key ? asked.logs : step;

  const answer = useInfiniteQuery({
    queryKey: ['logs', 'pattern-sample', key],
    enabled: query !== null,
    initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => {
      if (!query) throw new Error('No query to read patterns from.');

      return runLogCursorQuery({
        stream: query.stream,
        statement: query.statement,
        globalFilters: query.globalFilters,
        timeRange: query.timeRange,
        pageSize: query.step,
        cursor: pageParam ?? undefined,
      });
    },
    getNextPageParam: (last) => last.page.next_cursor ?? null,
    // What was read stays read: the logs of a query that has run do not change under the reader.
    staleTime: Infinity,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
  const { data, fetchNextPage, hasNextPage, isError, isFetching, refetch } = answer;

  const answers = data?.pages;
  const read = React.useMemo(() => (answers ?? []).reduce((sum, { page }) => sum + page.items.length, 0), [answers]);
  // One result of all the answers, so that a field only older logs carry is a column of it too.
  const result = React.useMemo(
    () => (answers && read > 0 ? recordsToQueryResult(answers.flatMap(({ page }) => page.items)) : undefined),
    [answers, read],
  );
  // An answer with nothing in it is the end, whatever it says of a next one.
  const more = hasNextPage && (answers?.at(-1)?.page.items.length ?? 0) > 0;
  const behind = more && read < wanted;

  React.useEffect(() => {
    if (behind && !isFetching && !isError) void fetchNextPage();
  }, [behind, fetchNextPage, isError, isFetching, read]);

  const readMore = React.useCallback(() => setAsked({ key, logs: read + step }), [key, read, step]);
  const retry = React.useCallback(() => {
    // With answers in hand the one that failed is asked again; with none, the first.
    void (read > 0 ? fetchNextPage() : refetch());
  }, [fetchNextPage, read, refetch]);

  return {
    result,
    reading: query !== null && !isError && (isFetching || behind),
    more,
    failed: isError,
    readMore,
    retry,
  };
}
