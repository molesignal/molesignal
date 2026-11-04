import { http } from '@/lib/http';

export interface TopologyNode {
  id: string;
  name: string;
  errorRate: number;
  p95Ms: number;
  rps: number;
  spanCount: number;
}

export interface TopologyEdge {
  source: string;
  target: string;
  rps: number;
  errRate: number;
  p95Ms: number;
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
