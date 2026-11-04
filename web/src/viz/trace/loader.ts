import { useQuery } from '@tanstack/react-query';

import * as webApi from '@/api/web';

/**
 * A trace's spans. `window` says when its spans began, where the place that
 * opens the trace knows it; without it the server finds a trace of the last 31
 * days by its summary.
 */
export function useTrace(traceId: string | undefined, window?: webApi.TraceWindow) {
  return useQuery({
    queryKey: ['web', 'trace', traceId, window?.from ?? null, window?.to ?? null],
    queryFn: () => webApi.trace(traceId!, window),
    enabled: !!traceId,
    staleTime: 60_000,
  });
}
