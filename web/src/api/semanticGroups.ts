import { http } from '@/lib/http';

export type MatchOp = 'eq' | 'neq' | 're' | 'nre';

export interface LabelMatcher {
  label: string;
  op: MatchOp;
  value: string;
}

export interface SemanticGroup {
  id: string;
  org_id: string;
  name: string;
  enabled: boolean;
  matchers: LabelMatcher[];
  group_by: string[];
  created_at: number;
  updated_at: number;
}

export type SemanticGroupInput = {
  name: string;
  enabled: boolean;
  matchers: LabelMatcher[];
  group_by: string[];
};

export async function list(): Promise<SemanticGroup[]> {
  const { data } = await http.get<SemanticGroup[]>('/alerts/semantic_groups');
  return data;
}

export async function create(group: SemanticGroupInput): Promise<SemanticGroup> {
  const { data } = await http.post<SemanticGroup>('/alerts/semantic_groups', group);
  return data;
}

export async function update(id: string, group: SemanticGroupInput): Promise<SemanticGroup> {
  const { data } = await http.put<SemanticGroup>(`/alerts/semantic_groups/${id}`, group);
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/alerts/semantic_groups/${id}`);
}

export async function importGroups(groups: SemanticGroupInput[]): Promise<{ imported: number }> {
  const { data } = await http.post<{ imported: number }>('/alerts/semantic_groups/import', {
    groups,
  });
  return data;
}
