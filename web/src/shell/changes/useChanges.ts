import { useQuery } from '@tanstack/react-query';

import { type ChangeEntry, type ChangeListParams, changesApi, changesQueryKeys } from '@/api/changes';
import { useAuthStore } from '@/stores/auth';

const NONE: readonly ChangeEntry[] = [];

/**
 * The changes of a stretch of time the reader may read. Changes are an aside
 * to the page they mark: a list that cannot be read marks nothing, and the
 * page goes on without it.
 */
export function useChanges(params: ChangeListParams | null): readonly ChangeEntry[] {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const query = useQuery({
    queryKey: params ? changesQueryKeys.list(orgId, params) : ['changes', 'none'],
    queryFn: () => changesApi.list(params!),
    enabled: Boolean(orgId && params && params.to > params.from),
    retry: false,
    staleTime: 60_000,
  });

  // Anything but a list (a proxy's page, an empty object) marks nothing.
  return Array.isArray(query.data) ? query.data : NONE;
}

/**
 * The changes of an explorer's range, for its histogram: those of every
 * service, and annotations of the stream in view or of none.
 */
export function useRangeChanges(range: { start: number; end: number } | null, stream?: string): readonly ChangeEntry[] {
  return useChanges(range ? { from: range.start, to: range.end, ...(stream ? { stream } : {}) } : null);
}
