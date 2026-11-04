import { http } from '@/lib/http';

/** A list of sessions a team keeps to go through one after the other; of one application. */
export interface RumPlaylist {
  id: string;
  application: string;
  name: string;
  description: string;
  createdBy: string | null;
  createdAt: number;
  updatedAt: number;
  sessionCount: number;
}

/** A session of a playlist, with the moment worth opening it at. */
export interface RumPlaylistSession {
  sessionId: string;
  /** In microseconds since the epoch; none opens the session at its beginning. */
  atTime: number | null;
  addedBy: string | null;
  addedAt: number;
}

/** A playlist with its sessions, in the order they were added. */
export interface RumPlaylistDetail extends RumPlaylist {
  sessions: RumPlaylistSession[];
}

/** A session to put in a playlist. */
export interface RumPlaylistSessionInput {
  sessionId: string;
  atTime?: number;
}

export interface CreateRumPlaylist {
  application: string;
  name: string;
  description?: string;
  sessions?: RumPlaylistSessionInput[];
}

export interface UpdateRumPlaylist {
  name?: string;
  description?: string;
}

const path = (id: string) => `/rum/playlists/${encodeURIComponent(id)}`;

/** The playlists of the organization, or of one application, the one changed last first. */
export async function listPlaylists(application?: string, signal?: AbortSignal): Promise<RumPlaylist[]> {
  const { data } = await http.get<RumPlaylist[]>('/rum/playlists', {
    params: application ? { application } : {},
    ...(signal && { signal }),
  });

  return data;
}

export async function getPlaylist(id: string, signal?: AbortSignal): Promise<RumPlaylistDetail> {
  const { data } = await http.get<RumPlaylistDetail>(path(id), { ...(signal && { signal }) });

  return data;
}

export async function createPlaylist(body: CreateRumPlaylist): Promise<RumPlaylistDetail> {
  const { data } = await http.post<RumPlaylistDetail>('/rum/playlists', body);

  return data;
}

export async function updatePlaylist(id: string, body: UpdateRumPlaylist): Promise<RumPlaylistDetail> {
  const { data } = await http.patch<RumPlaylistDetail>(path(id), body);

  return data;
}

export async function deletePlaylist(id: string): Promise<void> {
  await http.delete(path(id));
}

/** Puts sessions in; one that is there takes the moment it is given and keeps its place. */
export async function addPlaylistSessions(id: string, sessions: RumPlaylistSessionInput[]): Promise<RumPlaylistDetail> {
  const { data } = await http.post<RumPlaylistDetail>(`${path(id)}/sessions`, { sessions });

  return data;
}

export async function removePlaylistSession(id: string, sessionId: string): Promise<RumPlaylistDetail> {
  const { data } = await http.delete<RumPlaylistDetail>(`${path(id)}/sessions/${encodeURIComponent(sessionId)}`);

  return data;
}

/** The playlists a session is in. */
export async function sessionPlaylists(sessionId: string, signal?: AbortSignal): Promise<RumPlaylist[]> {
  const { data } = await http.get<RumPlaylist[]>(`/rum/sessions/${encodeURIComponent(sessionId)}/playlists`, {
    ...(signal && { signal }),
  });

  return data;
}
