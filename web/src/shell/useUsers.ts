import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as usersApi from '@/api/users';
import { useAuthStore } from '@/stores/auth';

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
 *
 * `users` are the organization's members, the people a picker offers; without `org.members.read`
 * they come without an email. `byId` also knows whoever is signed in: root reaches an organization
 * without belonging to it.
 */
export function useUsers() {
  const { t } = useTranslation('common');
  const signedIn = useAuthStore((state) => state.ctx);
  const query = useQuery<usersApi.MemberName[]>({ queryKey: ['users'], queryFn: () => usersApi.list() });
  const users: UserLite[] = React.useMemo(
    () =>
      (query.data ?? []).map((u) => ({
        id: u.id,
        name: u.displayName || u.email || '',
        email: u.email ?? '',
        avatarUrl: u.avatarUrl ?? null,
      })),
    [query.data],
  );
  const signedInId = signedIn?.userId;
  const signedInEmail = signedIn?.email ?? '';
  const signedInName = signedIn?.displayName || signedInEmail;
  const byId = React.useMemo(() => {
    const known = new Map(users.map((u) => [u.id, u]));

    if (signedInId && signedInName && !known.has(signedInId)) {
      known.set(signedInId, { id: signedInId, name: signedInName, email: signedInEmail });
    }

    return known;
  }, [users, signedInId, signedInName, signedInEmail]);
  const pending = query.isPending;
  const unavailable = query.isError;
  const nameOf = React.useCallback(
    (id: string) => {
      if (!id) return '';

      const known = byId.get(id);

      if (known) return known.name;
      if (pending) return '…';

      // Without the list nobody can be called a non-member.
      return unavailable ? t('people.someone') : t('people.non_member');
    },
    [byId, pending, unavailable, t],
  );
  return {
    users,
    byId,
    nameOf,
    isLoading: query.isLoading,
    isError: query.isError,
    error: query.error,
  };
}
