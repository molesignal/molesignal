import { http } from '@/lib/http';

export interface TopologyNode {
  id: string;
  name: string;
  error_rate: number;
  p95_ms: number;
  rps: number;
  span_count: number;
}

export interface TopologyEdge {
  source: string;
  target: string;
  rps: number;
  err_rate: number;
  p95_ms: number;
}

export interface TopologySnapshot {
  nodes: TopologyNode[];
  edges: TopologyEdge[];
}

export async function get(fromIso: string, toIso: string): Promise<TopologySnapshot> {
  const { data } = await http.get<TopologySnapshot>('/web/topology', {
    params: { from: fromIso, to: toIso },
  });
  return data;
}
