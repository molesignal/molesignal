import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as dashboardsApi from '@/api/dashboards';

import { dashboardDefinitionFromApi } from './model';
import type { DashboardDefinition } from './schema';

/**
 * The dashboard a route names, as the engine reads it. A stored model the
 * engine cannot read is an error of its own, apart from the request failing.
 */
export function useDashboardDefinition(id: string | undefined) {
  const query = useQuery({
    queryKey: ['dashboards', 'get', id],
    queryFn: () => dashboardsApi.get(id!),
    enabled: Boolean(id),
  });
  const parsed = React.useMemo<{
    definition: DashboardDefinition | null;
    error: unknown;
  }>(() => {
    if (!query.data) return { definition: null, error: null };

    try {
      return { definition: dashboardDefinitionFromApi(query.data), error: null };
    } catch (error) {
      return { definition: null, error };
    }
  }, [query.data]);

  return { query, definition: parsed.definition, modelError: parsed.error };
}
