import { http } from '@/lib/http';

/** Mirrors `molesignal::domain::iam::Team`. */
export interface Team {
  id: string;
  org_id: string;
  name: string;
  /** User ids belonging to the team. */
  member_ids: string[];
}

export interface TeamInput {
  name: string;
  member_ids: string[];
}

export async function list(): Promise<Team[]> {
  const { data } = await http.get<Team[]>('/teams');
  return data;
}

export async function get(id: string): Promise<Team> {
  const { data } = await http.get<Team>(`/teams/${encodeURIComponent(id)}`);
  return data;
}

export async function create(input: TeamInput): Promise<Team> {
  const { data } = await http.post<Team>('/teams', input);
  return data;
}

export async function update(id: string, input: TeamInput): Promise<Team> {
  const { data } = await http.put<Team>(`/teams/${encodeURIComponent(id)}`, input);
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/teams/${encodeURIComponent(id)}`);
}

/** A Mole Agent resource that keeps a team from being deleted. */
export interface TeamAgentResource {
  kind: 'skill' | 'skill_repository' | 'mcp_server';
  id: string;
  name: string;
}

/**
 * The Mole Agent resources a refused team deletion lists: they have to be moved
 * or deleted first. Null when the error is something else.
 */
export function teamAgentResources(error: unknown): TeamAgentResource[] | null {
  const response = (error as { response?: { status?: number; data?: unknown } } | null)?.response;
  if (response?.status !== 409) return null;
  const data = response.data as { code?: unknown; resources?: unknown } | undefined;
  if (data?.code !== 'team_has_agent_resources' || !Array.isArray(data.resources)) return null;
  return data.resources as TeamAgentResource[];
}
