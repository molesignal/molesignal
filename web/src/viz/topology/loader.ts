import { useQuery } from '@tanstack/react-query';

import * as webApi from '@/api/web';

export function useTopology(from: string, to: string) {
  return useQuery({
    queryKey: ['web', 'topology', from, to],
    queryFn: () => webApi.topology(from, to),
    staleTime: 30_000,
    enabled: !!from && !!to,
  });
}
