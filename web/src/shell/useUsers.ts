import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as usersApi from '@/api/users';

/** Trimmed user shape for pickers and on-call displays. */
export interface UserLite {
  id: string;
  name: string;
  email: string;
  avatarUrl?: string | null;
}

/**
 * Shared `['users']` query + id→user lookup, reused by the schedule, escalation
 * and incident surfaces so they all resolve user ids to the same display name.
 */
export function useUsers() {
  const query = useQuery({ queryKey: ['users'], queryFn: () => usersApi.list() });
  const users: UserLite[] = React.useMemo(
    () =>
      (query.data ?? []).map((u) => ({
        id: u.id,
        name: u.display_name || u.email,
        email: u.email,
        avatarUrl: u.avatar_url ?? null,
      })),
    [query.data],
  );
  const byId = React.useMemo(() => new Map(users.map((u) => [u.id, u])), [users]);
  const nameOf = React.useCallback((id: string) => byId.get(id)?.name ?? id, [byId]);
  return {
    users,
    byId,
    nameOf,
    isLoading: query.isLoading,
    isError: query.isError,
    error: query.error,
  };
}
