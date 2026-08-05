import { useQuery } from '@tanstack/react-query';
import { Navigate } from 'react-router-dom';

import * as meApi from '@/api/me';
import { resolveDefaultHomeRoute } from '@/lib/homeRoute';
import { USER_PREFERENCES_QUERY_KEY } from '@/shell/PreferenceRuntime';
import { useAuthStore } from '@/stores/auth';

export function DefaultHomeRedirect() {
  const context = useAuthStore((state) => state.ctx);
  const token = useAuthStore((state) => state.token);
  const shouldLoadPreferences = Boolean(token);
  const preferencesQuery = useQuery({
    queryKey: USER_PREFERENCES_QUERY_KEY,
    queryFn: () => meApi.preferences(),
    enabled: shouldLoadPreferences,
    staleTime: 5 * 60_000,
  });
  if (shouldLoadPreferences && preferencesQuery.isPending) return null;

  const preferences =
    preferencesQuery.data ?? meApi.DEFAULT_USER_PREFERENCES;
  const destination = resolveDefaultHomeRoute(
    preferences.default_home_route,
    context?.user_id ?? '',
    context?.org_id ?? '',
  );
  return <Navigate to={destination} replace />;
}
