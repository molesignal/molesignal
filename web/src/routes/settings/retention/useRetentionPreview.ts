import { skipToken, useQuery } from '@tanstack/react-query';
import * as React from 'react';

import { isShortening, type RetentionPreviewState } from './model';

/** Long enough that typing 30 does not ask about 3 first. */
const SETTLE_MS = 300;

/**
 * What the days typed would delete, once they have stopped changing and only when they keep less
 * than what is stored. A longer retention deletes nothing more, so it asks nothing.
 */
export function useRetentionPreview<T>({
  queryKey,
  load,
  stored,
  days,
}: {
  queryKey: readonly string[];
  /** Left out where a retention has no preview. */
  load?: ((days: number, signal?: AbortSignal) => Promise<T>) | undefined;
  stored: number | undefined;
  /** The days to save; `null` while there are none. */
  days: number | null;
}): RetentionPreviewState<T> {
  const wanted = load && stored !== undefined && days !== null && isShortening(stored, days) ? days : null;
  const [settled, setSettled] = React.useState<number | null>(null);

  React.useEffect(() => {
    if (wanted === null) {
      setSettled(null);
      return undefined;
    }

    const timer = window.setTimeout(() => setSettled(wanted), SETTLE_MS);
    return () => window.clearTimeout(timer);
  }, [wanted]);

  const query = useQuery({
    queryKey: [...queryKey, stored, settled],
    queryFn:
      load && settled !== null && settled === wanted
        ? ({ signal }) => load(settled, signal)
        : skipToken,
    // The figures are of this moment: never kept for a later look, and not worth asking twice for
    // when the catalog could not answer, since Save then asks for the days typed back.
    staleTime: 0,
    gcTime: 0,
    retry: false,
  });

  if (wanted === null) return { status: 'idle' };
  if (settled !== wanted || query.isPending || query.isFetching) return { status: 'pending' };
  if (query.isError) return { status: 'failed', retry: () => void query.refetch() };
  return { status: 'ready', preview: query.data };
}
