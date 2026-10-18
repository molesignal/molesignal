import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import type { Chat } from '@/api/agent/chat';
import * as teamsApi from '@/api/teams';
import { useActionAccess } from '@/product/actionAccess';
import { useAuthStore } from '@/stores/auth';

/**
 * Which team a chat is for. A chat not started yet takes the team chosen in the
 * composer or with `/team`; a chat that has started keeps the one it was
 * started for. The teams offered are the person's own, and every team to an
 * administrator of Mole Agent (`agent.manage`).
 */
export function useChatTeam(chat: Chat | undefined) {
  const userId = useAuthStore((state) => state.ctx?.user_id ?? '');
  const manager = !useActionAccess({ permission: 'agent.manage' }).disabled;
  const teamsQ = useQuery({
    queryKey: ['teams'],
    queryFn: teamsApi.list,
    retry: false,
    staleTime: 5 * 60_000,
  });
  const [chosenTeamId, setChosenTeamId] = React.useState<string | null>(null);

  const allTeams = teamsQ.data;
  const teams = React.useMemo(
    () =>
      (allTeams ?? [])
        .filter((team) => manager || team.member_ids.includes(userId))
        .sort((left, right) => left.name.localeCompare(right.name)),
    [allTeams, manager, userId],
  );

  const teamId = chat ? (chat.scope === 'team' ? (chat.team_id ?? null) : null) : chosenTeamId;
  const team = teamId ? (allTeams ?? []).find((item) => item.id === teamId) : undefined;

  return {
    /** The teams a new chat can be for. */
    teams,
    teamId,
    team,
    /** A chat that has started keeps its team. */
    locked: Boolean(chat),
    setTeamId: setChosenTeamId,
    /** The names of the organization's teams, for the chats in the list. */
    teamName: (id: string | null | undefined) =>
      (id && (allTeams ?? []).find((item) => item.id === id)?.name) || null,
  };
}
