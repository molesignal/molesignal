import { keepPreviousData, useQuery } from '@tanstack/react-query';

import * as rumApi from '@/api/rum';
import type { AnalyticsStepKind, FunnelParams, PathsParams, RetentionParams } from '@/api/rum';
import { useAuthStore } from '@/stores/auth';

import { type OverviewRange, type OverviewScope, scopeFilters } from '../overview/model';

/** The values a field has over the range, the commonest first: what a step can be asked to look for. */
export function useSuggestions(kind: AnalyticsStepKind, field: string, scope: OverviewScope, range: OverviewRange) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'analytics', orgId, 'values', kind, field, scope, range],
    queryFn: ({ signal }) =>
      rumApi.facets({ type: kind, from: range.from, to: range.to, filters: scopeFilters(scope), field, limit: 50 }, signal),
    enabled: Boolean(orgId && scope.application),
    placeholderData: keepPreviousData,
    staleTime: 60_000,
  });
}

/** The funnel a question asks for; a question that is not asked yet stays idle. */
export function useFunnel(params: FunnelParams | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'analytics', orgId, 'funnel', params],
    queryFn: ({ signal }) => rumApi.funnel(params as FunnelParams, signal),
    enabled: Boolean(orgId && params),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}

export function usePaths(params: PathsParams | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'analytics', orgId, 'paths', params],
    queryFn: ({ signal }) => rumApi.paths(params as PathsParams, signal),
    enabled: Boolean(orgId && params),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}

export function useRetention(params: RetentionParams | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'analytics', orgId, 'retention', params],
    queryFn: ({ signal }) => rumApi.retention(params as RetentionParams, signal),
    enabled: Boolean(orgId && params),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}
