import { useQuery } from '@tanstack/react-query';

import * as rumApi from '@/api/rum';
import type { RumAggregateParams, RumSearchParams } from '@/api/rum';
import { useAuthStore } from '@/stores/auth';

/**
 * One read of the numbers of a value in its drawer. Unlike the reads of the page it does not keep
 * the numbers of the value that was open before while the next are on their way: they are another
 * value's.
 */
export function useDrawerAggregate(name: string, params: RumAggregateParams | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'performance', orgId, 'aggregate', name, params],
    queryFn: ({ signal }) => rumApi.aggregate(params as RumAggregateParams, signal),
    enabled: Boolean(orgId && params),
    staleTime: 30_000,
  });
}

/** The rows of the views a drawer names, to say when each was and who it was of. */
export function useViewRows(params: RumSearchParams | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'performance', orgId, 'rows', params],
    queryFn: () => rumApi.search(params as RumSearchParams),
    select: (page) => page.items,
    enabled: Boolean(orgId && params),
    staleTime: 30_000,
  });
}
