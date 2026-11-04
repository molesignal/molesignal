import { http } from '@/lib/http';

export interface AgentToolset {
  id: string;
  orgId: string;
  name: string;
  schema: unknown;
  enabled: boolean;
  createdAt: number;
  updatedAt: number;
}

interface AgentToolsetWire extends Omit<AgentToolset, 'createdAt' | 'updatedAt'> {
  createdAt?: number;
  updatedAt?: number;
}

export interface CreateAgentToolsetInput {
  name: string;
  schema?: unknown;
  enabled?: boolean;
}

export async function list(): Promise<AgentToolset[]> {
  const { data } = await http.get<AgentToolsetWire[]>('/agent/settings/toolsets');
  return data.map(normalize);
}

export async function create(input: CreateAgentToolsetInput): Promise<AgentToolset> {
  const { data } = await http.post<AgentToolsetWire>('/agent/settings/toolsets', input);
  return normalize(data);
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/agent/settings/toolsets/${encodeURIComponent(id)}`);
}

function normalize(row: AgentToolsetWire): AgentToolset {
  return {
    ...row,
    createdAt: row.createdAt ?? 0,
    updatedAt: row.updatedAt ?? 0,
  };
}
