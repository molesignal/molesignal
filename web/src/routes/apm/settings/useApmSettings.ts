import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { apmQueryKeys } from '@/api/apm';
import { type ApmSettings, apmSettingsApi, apmSettingsQueryKeys, DEFAULT_APM_SETTINGS } from '@/api/apmSettings';
import { useAuthStore } from '@/stores/auth';

export function useApmSettingsQuery() {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: apmSettingsQueryKeys.settings(orgId),
    queryFn: () => apmSettingsApi.get(),
    enabled: Boolean(orgId),
    staleTime: 60_000,
  });
}

/** The organization's APM settings: the defaults until they are read, or when they cannot be. */
export function useApmSettings(): ApmSettings {
  return useApmSettingsQuery().data ?? DEFAULT_APM_SETTINGS;
}

/** Saves the settings; every APM figure is read again, since health, Apdex and aliases change them. */
export function useSaveApmSettings() {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const client = useQueryClient();

  return useMutation({
    mutationFn: (settings: ApmSettings) => apmSettingsApi.save(settings),
    onSuccess: (saved) => {
      client.setQueryData(apmSettingsQueryKeys.settings(orgId), saved);
      void client.invalidateQueries({
        queryKey: apmQueryKeys.all,
        predicate: (query) => query.queryKey[1] !== 'settings',
      });
    },
  });
}
