import { useQuery } from '@tanstack/react-query';

import * as rumApi from '@/api/rum';
import type { RumAggregateParams } from '@/api/rum';
import { useAuthStore } from '@/stores/auth';

/**
 * One read of the page. It keeps the numbers it had while the next are on their way, so a range that
 * moves on does not empty the card, but only when the next are of the same thing (`kind`): the numbers
 * of another level or another dimension are not these, and never stand in their place. A read that is
 * not asked for stays idle.
 */
export function useKeptAggregate(name: string, kind: string, params: RumAggregateParams | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'performance', orgId, name, kind, params],
    queryFn: ({ signal }) => rumApi.aggregate(params as RumAggregateParams, signal),
    enabled: Boolean(orgId && params),
    placeholderData: (previous, previousQuery) => (previousQuery?.queryKey[4] === kind ? previous : undefined),
    staleTime: 30_000,
  });
}
