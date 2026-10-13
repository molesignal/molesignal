import * as React from 'react';

import * as webApi from '@/api/web';
import { answerFromPayload, type HistogramCountsSource } from '@/shell/query/histogram/counts';
import type { GlobalFilter } from '@/stores/useFiltersStore';

import { logMatchRequest } from './cursorQuery';

/** The field query whose matches the histogram counts, as it was run. */
export interface CountedLogQuery {
  orgId: string;
  stream: string;
  statement: string;
  globalFilters: GlobalFilter[];
  /** When the query was run: running it again counts it again. */
  ranAt: number;
}

/**
 * Where the Logs histogram gets the count of every row a field query matches.
 *
 * The count is asked with the request the list itself sends, so it is of the
 * rows the list pages through. A statement the reader wrote has no such
 * request (`null`): its histogram is of the rows it returned.
 */
export function useLogHistogramCounts(query: CountedLogQuery | null): HistogramCountsSource | undefined {
  return React.useMemo(() => {
    if (!query) return undefined;

    const { orgId, stream, statement, globalFilters, ranAt } = query;

    return {
      key: ['logs', orgId, stream, statement, JSON.stringify(globalFilters), ranAt],
      load: async ({ range, binMicros, originMicros }, signal) =>
        answerFromPayload(
          await webApi.logsHistogram(
            {
              ...logMatchRequest(stream, statement, globalFilters, range),
              bucket_micros: binMicros,
              origin_micros: originMicros,
            },
            signal,
          ),
        ),
    };
  }, [query]);
}
