import { http } from '@/lib/http';

import type { ApmMeta, ApmQueryParams, DependencyIdentity, ErrorSummary, RedSummary, ServiceHealth, TransactionIdentity, TrendPoint } from './apm';

export type MapNodeKind = 'service' | 'external' | 'topic';
export type MapEdgeKind = 'call' | 'publish' | 'consume';
/** Against the stretch of the same length before: `new` when it was not there, `gone` when it is no longer. */
export type MapChange = 'new' | 'gone';

export interface MapRed {
  requestCount: number;
  errorCount: number;
  errorRate: number;
  p95Time?: number | null;
}

export interface MapNode {
  /** `service:<name>`, `topic:<name>`, or `<category>:<target>`. */
  id: string;
  kind: MapNodeKind;
  name: string;
  namespace?: string;
  category?: DependencyIdentity['category'];
  /** The environments a service ran in. */
  environments?: string[];
  /** What a service served; what an external node or a topic was asked. */
  red: MapRed;
  health?: ServiceHealth;
  change?: MapChange;
}

export interface MapEdge {
  id: string;
  source: string;
  target: string;
  kind: MapEdgeKind;
  red: MapRed;
  change?: MapChange;
}

export interface MapResponse {
  meta: ApmMeta;
  nodes: MapNode[];
  edges: MapEdge[];
  /** The namespaces of the services in the environment in view. */
  namespaces: string[];
  /** Every environment the services run in, whichever is in view. */
  environments?: string[];
  /** More nodes than a map draws: the least busy services were left out. */
  truncated: boolean;
}

export interface MapOperation {
  name: string;
  kind?: TransactionIdentity['kind'];
  red: RedSummary;
}

export interface MapEdgeDetail {
  meta: ApmMeta;
  kind: MapEdgeKind;
  red: RedSummary;
  trend: TrendPoint[];
  operations: MapOperation[];
  /** The operations are the called service's endpoints, as every caller asked them. */
  everyCaller: boolean;
  errors: ErrorSummary[];
}

/** How long a topic's messages waited between being published and being consumed, from a sample. */
export interface MessagingLatency {
  sampled: number;
  matched: number;
  p50Time?: number | null;
  p95Time?: number | null;
  maxTime?: number | null;
  unavailable?: string;
}

function clean(params: Record<string, unknown>): Record<string, string | number> {
  return Object.fromEntries(Object.entries(params).filter(([, value]) => value !== undefined && value !== null && value !== '')) as Record<string, string | number>;
}

export const apmMapApi = {
  map: async (params: ApmQueryParams) => {
    const { data } = await http.get<MapResponse>('/apm/map', { params: clean({ ...params }) });
    return data;
  },
  edge: async (params: ApmQueryParams & { source: string; target: string }) => {
    const { data } = await http.get<MapEdgeDetail>('/apm/map/edge', { params: clean({ ...params }) });
    return data;
  },
  messagingLatency: async (params: { from: number; to: number; topic: string; consumer?: string }) => {
    const { data } = await http.get<MessagingLatency>('/web/traces/messaging-latency', { params: clean({ ...params }) });
    return data;
  },
};

export const apmMapQueryKeys = {
  map: (orgId: string, params: ApmQueryParams) => ['apm', 'map', orgId, params] as const,
  edge: (orgId: string, params: ApmQueryParams & { source: string; target: string }) => ['apm', 'map-edge', orgId, params] as const,
  messagingLatency: (orgId: string, params: { from: number; to: number; topic: string; consumer?: string }) =>
    ['apm', 'messaging-latency', orgId, params] as const,
};
