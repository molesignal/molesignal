import { useQuery } from '@tanstack/react-query';

import * as webApi from '@/api/web';

export function useTrace(traceId: string | undefined) {
  return useQuery({
    queryKey: ['web', 'trace', traceId],
    queryFn: () => webApi.trace(traceId!),
    enabled: !!traceId,
    staleTime: 60_000,
  });
}
