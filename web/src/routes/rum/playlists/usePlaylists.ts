import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import * as rumApi from '@/api/rum';
import type { RumPlaylistSessionInput, UpdateRumPlaylist } from '@/api/rum';
import { useAuthStore } from '@/stores/auth';

/** What the readers of playlists share, so that a change made in one place is seen in the others. */
const KEY = ['rum', 'playlists'] as const;

const useOrg = () => useAuthStore((state) => state.ctx?.orgId ?? '');

/** The playlists of the organization, or of one application. */
export function usePlaylists(application?: string, enabled = true) {
  const orgId = useOrg();

  return useQuery({
    queryKey: [...KEY, orgId, 'list', application ?? ''],
    queryFn: ({ signal }) => rumApi.listPlaylists(application, signal),
    enabled: Boolean(orgId) && enabled,
  });
}

/** One playlist with the sessions in it. */
export function usePlaylist(id: string) {
  const orgId = useOrg();

  return useQuery({
    queryKey: [...KEY, orgId, 'detail', id],
    queryFn: ({ signal }) => rumApi.getPlaylist(id, signal),
    enabled: Boolean(orgId && id),
    retry: false,
  });
}

/** The playlists a session is in. */
export function useSessionPlaylists(sessionId: string, enabled = true) {
  const orgId = useOrg();

  return useQuery({
    queryKey: [...KEY, orgId, 'session', sessionId],
    queryFn: ({ signal }) => rumApi.sessionPlaylists(sessionId, signal),
    enabled: Boolean(orgId && sessionId) && enabled,
    retry: false,
  });
}

/** What changes a playlist. Each one makes the readers of playlists read again. */
export function usePlaylistChanges() {
  const client = useQueryClient();
  const settled = () => client.invalidateQueries({ queryKey: KEY });

  return {
    create: useMutation({ mutationFn: rumApi.createPlaylist, onSuccess: settled }),
    update: useMutation({
      mutationFn: ({ id, ...body }: UpdateRumPlaylist & { id: string }) => rumApi.updatePlaylist(id, body),
      onSuccess: settled,
    }),
    remove: useMutation({ mutationFn: (id: string) => rumApi.deletePlaylist(id), onSuccess: settled }),
    add: useMutation({
      mutationFn: ({ id, sessions }: { id: string; sessions: RumPlaylistSessionInput[] }) => rumApi.addPlaylistSessions(id, sessions),
      onSuccess: settled,
    }),
    take: useMutation({
      mutationFn: ({ id, sessionId }: { id: string; sessionId: string }) => rumApi.removePlaylistSession(id, sessionId),
      onSuccess: settled,
    }),
  };
}
