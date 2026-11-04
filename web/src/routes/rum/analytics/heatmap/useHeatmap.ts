import { keepPreviousData, useQuery } from '@tanstack/react-query';

import * as rumApi from '@/api/rum';
import type { HeatmapParams } from '@/api/rum';
import { useAuthStore } from '@/stores/auth';

export function useClickMap(params: HeatmapParams | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'heatmap', orgId, 'clicks', params],
    queryFn: ({ signal }) => rumApi.heatmapClicks(params as HeatmapParams, signal),
    enabled: Boolean(orgId && params),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}

export function useScrollMap(params: HeatmapParams | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'heatmap', orgId, 'scroll', params],
    queryFn: ({ signal }) => rumApi.heatmapScroll(params as HeatmapParams, signal),
    enabled: Boolean(orgId && params),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}

/** A recording of the page to draw the map over. One the reader may not see leaves the numbers without it. */
export function useSnapshot(params: HeatmapParams | null, view: string | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'heatmap', orgId, 'snapshot', params?.filters, params?.from, params?.to, view],
    queryFn: ({ signal }) =>
      rumApi.heatmapSnapshot(
        { from: params?.from as number, to: params?.to as number, filters: params?.filters ?? [], ...(view ? { viewId: view } : {}) },
        signal,
      ),
    enabled: Boolean(orgId && params),
    retry: false,
    staleTime: 60_000,
  });
}
