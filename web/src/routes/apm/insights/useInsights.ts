import { useQuery } from '@tanstack/react-query';

import type { ApmQueryParams } from '@/api/apm';
import { apmInsightsApi, apmInsightsQueryKeys } from '@/api/apmInsights';

/** The insights of the range for the services `params` keep in view. */
export function useInsights(orgId: string, params: ApmQueryParams) {
  return useQuery({
    queryKey: apmInsightsQueryKeys.insights(orgId, params),
    queryFn: () => apmInsightsApi.insights(params),
    enabled: Boolean(orgId),
    refetchInterval: 60_000,
  });
}
