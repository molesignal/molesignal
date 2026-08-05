import { http } from '@/lib/http';

export type NetworkAccess = 'blocked' | 'allowed';

export interface AgentProfile {
  id: string;
  name: string;
  description: string;
  model_provider_id?: string | null;
  model?: string | null;
  allowed_tools: string[];
  data_scope: Record<string, unknown>;
  risk_policy: Record<string, unknown>;
  network_access: NetworkAccess;
  max_context_tokens: number;
  max_investigation_secs: number;
  max_tool_calls: number;
  is_default: boolean;
  enabled: boolean;
  created_by: string;
  created_at: number;
  updated_at: number;
}

export interface AgentProfileInput {
  name: string;
  description: string;
  model_provider_id?: string | null;
  model?: string | null;
  allowed_tools: string[];
  data_scope: Record<string, unknown>;
  risk_policy: Record<string, unknown>;
  network_access: NetworkAccess;
  max_context_tokens: number;
  max_investigation_secs: number;
  max_tool_calls: number;
  is_default: boolean;
  enabled: boolean;
}

export async function listProfiles(): Promise<AgentProfile[]> {
  const { data } =
    await http.get<{ profiles: AgentProfile[] }>('/intelligence/settings/agent-profiles');
  return data.profiles ?? [];
}

export async function createProfile(input: AgentProfileInput): Promise<AgentProfile> {
  const { data } = await http.post<AgentProfile>('/intelligence/settings/agent-profiles', input);
  return data;
}

export async function updateProfile(
  id: string,
  input: AgentProfileInput,
): Promise<AgentProfile> {
  const { data } = await http.put<AgentProfile>(
    `/intelligence/settings/agent-profiles/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}
