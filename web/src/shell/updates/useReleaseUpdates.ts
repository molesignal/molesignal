import { useQuery } from '@tanstack/react-query';

import * as versionApi from '@/api/version';
import { useAuthStore } from '@/stores/auth';

export const RELEASE_UPDATES_KEY = ['release-updates'] as const;

export function useReleaseUpdates(enabled = true) {
  const userId = useAuthStore((s) => s.ctx?.userId);
  return useQuery({
    queryKey: RELEASE_UPDATES_KEY,
    queryFn: () => versionApi.updates(),
    enabled: enabled && !!userId,
    staleTime: 5 * 60_000,
    refetchInterval: 6 * 60 * 60_000,
    retry: false,
  });
}
