import { http } from '@/lib/http';

export type NetworkAccess = 'blocked' | 'allowed';

export interface AgentProfile {
  id: string;
  name: string;
  description: string;
  modelProviderId?: string | null;
  model?: string | null;
  allowedTools: string[];
  dataScope: Record<string, unknown>;
  riskPolicy: Record<string, unknown>;
  networkAccess: NetworkAccess;
  maxContextTokens: number;
  maxInvestigationSecs: number;
  maxToolCalls: number;
  /** Sampling temperature; unset uses Mole Agent's default (0.2). */
  temperature?: number | null;
  /** Chats using it may pick Bypass permissions: nothing is asked in the chat, approvals stay. */
  allowBypassPermissions?: boolean;
  isDefault: boolean;
  enabled: boolean;
  createdBy: string;
  createdAt: number;
  updatedAt: number;
}

export interface AgentProfileInput {
  name: string;
  description: string;
  modelProviderId?: string | null;
  model?: string | null;
  allowedTools: string[];
  dataScope: Record<string, unknown>;
  riskPolicy: Record<string, unknown>;
  networkAccess: NetworkAccess;
  maxContextTokens: number;
  maxInvestigationSecs: number;
  maxToolCalls: number;
  temperature?: number | null;
  allowBypassPermissions?: boolean;
  isDefault: boolean;
  enabled: boolean;
}

export async function listProfiles(): Promise<AgentProfile[]> {
  const { data } =
    await http.get<{ profiles: AgentProfile[] }>('/agent/settings/agent-profiles');
  return data.profiles ?? [];
}

export async function createProfile(input: AgentProfileInput): Promise<AgentProfile> {
  const { data } = await http.post<AgentProfile>('/agent/settings/agent-profiles', input);
  return data;
}

export async function updateProfile(
  id: string,
  input: AgentProfileInput,
): Promise<AgentProfile> {
  const { data } = await http.put<AgentProfile>(
    `/agent/settings/agent-profiles/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}
