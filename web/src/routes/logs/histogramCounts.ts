import * as React from 'react';

import * as webApi from '@/api/web';
import { answerFromPayload, type HistogramCountsSource } from '@/shell/query/histogram/counts';

import type { PinnedLogFilter } from './conditions/model';
import { logMatchRequest } from './cursorQuery';

/** The field query whose matches the histogram counts, as it was run. */
export interface CountedLogQuery {
  orgId: string;
  stream: string;
  statement: string;
  /** The pinned filters as they applied to the stream when it ran. */
  pinned: readonly PinnedLogFilter[];
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

    const { orgId, stream, statement, pinned, ranAt } = query;

    return {
      key: ['logs', orgId, stream, statement, JSON.stringify(pinned), ranAt],
      load: async ({ range, binMicros, originTime: originMicros }, signal) =>
        answerFromPayload(
          await webApi.logsHistogram(
            {
              ...logMatchRequest(stream, statement, pinned, range),
              bucketTime: binMicros,
              originTime: originMicros,
            },
            signal,
          ),
        ),
    };
  }, [query]);
}
