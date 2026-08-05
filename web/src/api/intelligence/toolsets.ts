import { http } from '@/lib/http';

export interface AgentToolset {
  id: string;
  org_id: string;
  name: string;
  schema: unknown;
  enabled: boolean;
  created_at_micros: number;
  updated_at_micros: number;
}

interface AgentToolsetWire extends Omit<AgentToolset, 'created_at_micros' | 'updated_at_micros'> {
  created_at?: number;
  updated_at?: number;
  created_at_micros?: number;
  updated_at_micros?: number;
}

export interface CreateAgentToolsetInput {
  name: string;
  schema?: unknown;
  enabled?: boolean;
}

export async function list(): Promise<AgentToolset[]> {
  const { data } = await http.get<AgentToolsetWire[]>('/intelligence/settings/toolsets');
  return data.map(normalize);
}

export async function create(input: CreateAgentToolsetInput): Promise<AgentToolset> {
  const { data } = await http.post<AgentToolsetWire>('/intelligence/settings/toolsets', input);
  return normalize(data);
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/intelligence/settings/toolsets/${encodeURIComponent(id)}`);
}

function normalize(row: AgentToolsetWire): AgentToolset {
  return {
    ...row,
    created_at_micros: row.created_at_micros ?? row.created_at ?? 0,
    updated_at_micros: row.updated_at_micros ?? row.updated_at ?? 0,
  };
}
