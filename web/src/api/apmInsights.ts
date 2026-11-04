import { http } from '@/lib/http';

import type { ApmMeta, ApmQueryParams, ServiceHealth, ServiceIdentity } from './apm';
import type { DeploymentStatus } from './apmDeployments';

export type InsightKind = 'anomaly' | 'regression' | 'slow_dependency' | 'repeated_call' | 'slow_query' | 'hot_endpoint';
export type InsightSeverity = 'critical' | 'warning' | 'info';
export type ChangeKind = 'deployment' | 'configuration' | 'feature_flag';

/** A change near when a move began: reported by a pipeline or a tool, or a version spans first carried then. */
export interface NearbyChange {
  kind: ChangeKind;
  source: 'reported' | 'observed';
  service: string;
  /** Empty for a change that names no version. */
  version: string;
  status: DeploymentStatus;
  at: number;
  /** A service the moving one calls, rather than itself. */
  downstream: boolean;
  commit?: string;
  description?: string;
}

/** Something APM finds worth a look, with the figures behind it. */
export interface Insight {
  kind: InsightKind;
  severity: InsightSeverity;
  service: ServiceIdentity;
  transaction?: { name: string; kind: string };
  dependency?: { category: string; target: string; operation?: string };
  /** What moved or got worse. */
  metric?: 'error_rate' | 'p95' | 'throughput';
  /** The error rate, the P95 in microseconds or the requests a second; a share of time; the calls a request. */
  value: number;
  /** The same figure over the stretch before, and over the same stretch a week before. */
  previous?: number;
  lastWeek?: number;
  /** When the move began. */
  since?: number;
  version?: string;
  previousVersion?: string;
  calls?: number;
  p95Time?: number;
  changes?: NearbyChange[];
}

export interface BaselineFigures {
  requestCount: number;
  throughput: number;
  errorRate: number;
  p95Time?: number;
  apdex?: number;
}

export interface InsightsResponse {
  meta: ApmMeta;
  now: BaselineFigures;
  previous?: BaselineFigures;
  lastWeek?: BaselineFigures;
  insights: Insight[];
}

export interface MapRedFigures {
  requestCount: number;
  errorCount: number;
  errorRate: number;
  p95Time?: number;
}

/** A node a service reaches along the map. */
export interface BlastNode {
  id: string;
  kind: 'service' | 'external' | 'topic';
  name: string;
  namespace?: string;
  category?: string;
  hops: number;
  /** The node a hop nearer the service. */
  via: string;
  health: ServiceHealth;
  red: MapRedFigures;
  /** The calls between it and `via`. */
  call: MapRedFigures;
}

export interface BlastRadius {
  meta: ApmMeta;
  service?: BlastNode;
  /** Who calls the service: what its trouble reaches. */
  upstream: BlastNode[];
  /** What it calls: where its trouble may come from, the unhealthy first. */
  downstream: BlastNode[];
}

function clean(params: ApmQueryParams): Record<string, string | number> {
  return Object.fromEntries(Object.entries(params).filter(([, value]) => value !== undefined && value !== null && value !== '')) as Record<string, string | number>;
}

export const apmInsightsApi = {
  insights: async (params: ApmQueryParams) => {
    const { data } = await http.get<InsightsResponse>('/apm/insights', { params: clean(params) });
    return data;
  },
  blastRadius: async (params: ApmQueryParams & { service: string }) => {
    const { data } = await http.get<BlastRadius>('/apm/blast-radius', { params: clean(params) });
    return data;
  },
};

export const apmInsightsQueryKeys = {
  insights: (orgId: string, params: ApmQueryParams) =>
    ['apm', 'insights', orgId, params.from ?? null, params.to ?? null, params.namespace ?? null, params.service ?? null, params.environment ?? null] as const,
  blastRadius: (orgId: string, params: ApmQueryParams & { service: string }) =>
    ['apm', 'blast-radius', orgId, params.service, params.from ?? null, params.to ?? null, params.environment ?? null] as const,
};
