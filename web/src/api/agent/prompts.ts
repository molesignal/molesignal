import { http } from '@/lib/http';

export type PromptScope = 'builtin' | 'org' | 'user';
export type PromptPurpose =
  | 'system'
  | 'anomaly_analysis'
  | 'root_cause'
  | 'alert_explain'
  | 'query_generation'
  | 'dashboard_authoring'
  | 'evaluation';

export interface AgentPrompt {
  id: string;
  orgId?: string | null;
  userId?: string | null;
  scope: PromptScope;
  builtinKey?: string | null;
  purpose: PromptPurpose;
  name: string;
  body: string;
  variablesSchema: Record<string, unknown>;
  isDefault: boolean;
  enabled: boolean;
  version: number;
  parentId?: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface CreatePromptInput {
  scope: 'org' | 'user';
  purpose: PromptPurpose;
  builtinKey?: string | undefined;
  name: string;
  body: string;
  variablesSchema?: Record<string, unknown> | undefined;
  parentId?: string | undefined;
  enabled?: boolean | undefined;
}

export interface UpdatePromptInput {
  name: string;
  body: string;
  variablesSchema?: Record<string, unknown> | undefined;
  enabled?: boolean | undefined;
}

export async function list(): Promise<AgentPrompt[]> {
  const { data } = await http.get<AgentPrompt[]>('/agent/settings/prompts');
  return data;
}

export async function create(input: CreatePromptInput): Promise<AgentPrompt> {
  const { data } = await http.post<AgentPrompt>('/agent/settings/prompts', input);
  return data;
}

export async function update(id: string, input: UpdatePromptInput): Promise<AgentPrompt> {
  const { data } = await http.put<AgentPrompt>(
    `/agent/settings/prompts/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}

/**
 * Make a prompt the default. An organization prompt waits for its check first: `confirm`
 * sets it anyway when the organization lets a missing or worse check be confirmed.
 */
export async function setDefault(id: string, confirm = false): Promise<AgentPrompt> {
  const { data } = await http.post<AgentPrompt>(
    `/agent/settings/prompts/${encodeURIComponent(id)}/set-default`,
    { confirm },
  );
  return data;
}

export async function restore(id: string): Promise<AgentPrompt> {
  const { data } = await http.post<AgentPrompt>(
    `/agent/settings/prompts/${encodeURIComponent(id)}/restore`,
  );
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/agent/settings/prompts/${encodeURIComponent(id)}`);
}
