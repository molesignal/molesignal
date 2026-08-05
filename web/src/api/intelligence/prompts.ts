import { http } from '@/lib/http';

export type PromptScope = 'builtin' | 'org' | 'user';
export type PromptPurpose =
  | 'system'
  | 'anomaly_analysis'
  | 'root_cause'
  | 'alert_explain'
  | 'query_generation'
  | 'dashboard_authoring';

export interface AgentPrompt {
  id: string;
  org_id?: string | null;
  user_id?: string | null;
  scope: PromptScope;
  builtin_key?: string | null;
  purpose: PromptPurpose;
  name: string;
  body: string;
  variables_schema: Record<string, unknown>;
  is_default: boolean;
  enabled: boolean;
  version: number;
  parent_id?: string | null;
  created_at_micros: number;
  updated_at_micros: number;
}

export interface CreatePromptInput {
  scope: 'org' | 'user';
  purpose: PromptPurpose;
  builtin_key?: string | undefined;
  name: string;
  body: string;
  variables_schema?: Record<string, unknown> | undefined;
  parent_id?: string | undefined;
  enabled?: boolean | undefined;
}

export interface UpdatePromptInput {
  name: string;
  body: string;
  variables_schema?: Record<string, unknown> | undefined;
  enabled?: boolean | undefined;
}

export async function list(): Promise<AgentPrompt[]> {
  const { data } = await http.get<AgentPrompt[]>('/intelligence/settings/prompts');
  return data;
}

export async function create(input: CreatePromptInput): Promise<AgentPrompt> {
  const { data } = await http.post<AgentPrompt>('/intelligence/settings/prompts', input);
  return data;
}

export async function update(id: string, input: UpdatePromptInput): Promise<AgentPrompt> {
  const { data } = await http.put<AgentPrompt>(
    `/intelligence/settings/prompts/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}

export async function setDefault(id: string): Promise<AgentPrompt> {
  const { data } = await http.post<AgentPrompt>(
    `/intelligence/settings/prompts/${encodeURIComponent(id)}/set-default`,
  );
  return data;
}

export async function restore(id: string): Promise<AgentPrompt> {
  const { data } = await http.post<AgentPrompt>(
    `/intelligence/settings/prompts/${encodeURIComponent(id)}/restore`,
  );
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/intelligence/settings/prompts/${encodeURIComponent(id)}`);
}
