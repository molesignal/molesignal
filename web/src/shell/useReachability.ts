import { useQuery } from '@tanstack/react-query';

import * as notifyApi from '@/api/notify';

/** At most this many people per request, as the server takes them. */
const MAX_USERS = 200;

/**
 * Whether these people can get alerts, by id; empty while it loads or when it cannot be read,
 * so nothing is flagged by mistake.
 */
export function useReachability(userIds: readonly string[]): Record<string, notifyApi.Reachability> {
  const ids = [...new Set(userIds.filter(Boolean))].sort().slice(0, MAX_USERS);
  const query = useQuery({
    queryKey: ['notify-reachability', ids],
    queryFn: () => notifyApi.reachability(ids),
    enabled: ids.length > 0,
    staleTime: 60_000,
    retry: false,
  });
  return query.data ?? {};
}
