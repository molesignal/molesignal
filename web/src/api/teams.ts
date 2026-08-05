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
