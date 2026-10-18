import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as teamsApi from '@/api/teams';
import { useActionAccess } from '@/product/actionAccess';
import { useAuthStore } from '@/stores/auth';

/**
 * Whose a Mole Agent resource is (a skill, a repository, an MCP server): the
 * organization's, a team's or one person's. The rules are the backend's
 * (`crates/modules/agent/src/scope.rs`): organization resources are changed by
 * `agent.manage`, a team's by its members and administrators, a personal one
 * by its owner only.
 */
export interface ScopeTarget {
  scope: 'org' | 'team' | 'personal';
  team_id?: string;
}

/** A scope as one value for a select: `org`, `personal` or `team:<id>`. */
export function scopeChoice(target: {
  scope: string;
  team_id?: string | null | undefined;
}): string {
  if (target.scope === 'team' && target.team_id) return `team:${target.team_id}`;
  return target.scope === 'org' ? 'org' : 'personal';
}

export function scopeTarget(choice: string): ScopeTarget {
  if (choice.startsWith('team:')) return { scope: 'team', team_id: choice.slice('team:'.length) };
  return { scope: choice === 'org' ? 'org' : 'personal' };
}

/**
 * The scopes the signed-in person can put resources in, best first: their own,
 * their teams' (every team for an administrator), the organization's for an
 * administrator. Also how to name a scope a resource is in.
 */
export function useScopes() {
  const { t } = useTranslation('agent');
  const userId = useAuthStore((state) => state.ctx?.user_id ?? '');
  const manager = !useActionAccess({ permission: 'agent.manage' }).disabled;
  const teamsQ = useQuery({
    queryKey: ['teams'],
    queryFn: teamsApi.list,
    retry: false,
    staleTime: 5 * 60_000,
  });
  const allTeams = teamsQ.data;

  const options = React.useMemo(() => {
    const teams = (allTeams ?? [])
      .filter((team) => manager || team.member_ids.includes(userId))
      .sort((left, right) => left.name.localeCompare(right.name));
    return [
      { value: 'personal', label: t('scope.personal') },
      ...teams.map((team) => ({ value: `team:${team.id}`, label: team.name })),
      ...(manager ? [{ value: 'org', label: t('scope.org') }] : []),
    ];
  }, [allTeams, manager, t, userId]);

  /** What a scope is called on a resource: the team's name, or whose personal resource it is. */
  const label = React.useCallback(
    (scope: { scope: string; team_id?: string | null; owner_id?: string | null }) => {
      if (scope.scope === 'org') return t('scope.org');
      if (scope.scope === 'instance') return t('scope.instance');
      if (scope.scope === 'team') {
        return (
          (allTeams ?? []).find((team) => team.id === scope.team_id)?.name ?? t('scope.team')
        );
      }
      return !scope.owner_id || scope.owner_id === userId
        ? t('scope.personal')
        : t('scope.member');
    },
    [allTeams, t, userId],
  );

  return { options, manager, userId, label };
}
