import { http } from '@/lib/http';

export type ApmResolution = 'auto' | 'minute' | 'hour';
export type SortDirection = 'asc' | 'desc';

export interface TimeRange {
  from: number;
  to: number;
}

export interface ProjectionGap {
  org_id: string;
  range: { start: number; end: number };
  reason:
    | 'queue_full'
    | 'repository_unavailable'
    | 'flush_failed'
    | 'late_dropped'
    | 'cardinality_rejected'
    | 'shutdown_timeout';
  dropped_facts: number;
  recorded_at: number;
}

export interface DataQuality {
  partial: boolean;
  gaps: ProjectionGap[];
  overflow_dimensions: string[];
}

export interface ApmMeta {
  range: TimeRange;
  resolution: Exclude<ApmResolution, 'auto'>;
  projection_started_at?: number;
  last_complete_bucket_at?: number;
  data_quality: DataQuality;
  activation_boundary: boolean;
}

export interface TraceExemplar {
  trace_id: string;
  span_id: string;
  event_time: number;
  duration_micros: number;
  trace_available: boolean;
}

export interface RedSummary {
  request_count: number;
  error_count: number;
  error_rate: number;
  duration_sum_micros: number;
  duration_average_micros?: number;
  p50_micros?: number;
  p95_micros?: number;
  p99_micros?: number;
  latency_partial: boolean;
  exemplars: TraceExemplar[];
}

export interface TrendPoint {
  bucket_at: number;
  red: RedSummary;
}

export interface ServiceIdentity {
  namespace: string;
  name: string;
  environment: string;
}

export interface SignalFilterHandle {
  namespace: string;
  service: string;
  environment: string;
  version?: string;
  transaction?: string;
  dependency?: string;
  error_fingerprint?: string;
  from: number;
  to: number;
}

export interface ServiceSummary {
  service: ServiceIdentity;
  first_seen_at: number;
  last_seen_at: number;
  instrumentation: {
    runtime_language?: string;
    telemetry_sdk_name?: string;
    telemetry_sdk_version?: string;
    recent_instance_count: number;
  };
  versions: string[];
  red: RedSummary;
  health: 'healthy' | 'warning' | 'critical' | 'no_traffic';
  traces: SignalFilterHandle;
}

export interface TransactionIdentity {
  name: string;
  kind: 'http' | 'rpc' | 'messaging' | 'span' | 'other';
}

export interface DependencyIdentity {
  category:
    | 'service'
    | 'database'
    | 'cache'
    | 'messaging'
    | 'external_http'
    | 'external_rpc'
    | 'other';
  target: string;
  operation?: string;
}

export interface ErrorIdentity {
  fingerprint: string;
  error_type: string;
  application_frame?: string;
  transaction_name?: string;
  overflow: boolean;
}

export interface TransactionSummary {
  service: ServiceIdentity;
  version?: string;
  transaction: TransactionIdentity;
  red: RedSummary;
  total_time_micros: number;
  traces: SignalFilterHandle;
}

export interface DependencySummary {
  service: ServiceIdentity;
  version?: string;
  dependency: DependencyIdentity;
  red: RedSummary;
  total_time_micros: number;
  traces: SignalFilterHandle;
}

export interface ErrorSummary {
  error: ErrorIdentity;
  service: ServiceIdentity;
  first_seen_at: number;
  last_seen_at: number;
  occurrence_count: number;
  representative_message?: string;
  red: RedSummary;
  traces: SignalFilterHandle;
}

export interface VersionSummary {
  service: ServiceIdentity;
  version: string;
  first_seen_at: number;
  last_seen_at: number;
  observation_count: number;
}

export interface PagedResponse<T> {
  meta: ApmMeta;
  items: T[];
  next_cursor: string | null;
  previous_cursor: string | null;
  has_more: boolean;
  sort: string;
}

export interface OverviewResponse {
  meta: ApmMeta;
  red: RedSummary;
  trend: TrendPoint[];
  service_health: {
    healthy: number;
    warning: number;
    critical: number;
    no_traffic: number;
  };
  services: ServiceSummary[];
  top_transactions: TransactionSummary[];
  top_dependencies: DependencySummary[];
  top_errors: ErrorSummary[];
  recent_versions: VersionSummary[];
}

export interface ServiceDetailResponse {
  meta: ApmMeta;
  service: ServiceSummary;
  red: RedSummary;
  trend: TrendPoint[];
  transactions: TransactionSummary[];
  dependencies: DependencySummary[];
  errors: ErrorSummary[];
  versions: VersionSummary[];
}

export interface TransactionDetailResponse {
  meta: ApmMeta;
  transaction: TransactionSummary;
  trend: TrendPoint[];
  errors: ErrorSummary[];
  versions: VersionSummary[];
}

export interface ErrorSample {
  event_time: number;
  trace_id: string;
  span_id: string;
  trace_available: boolean;
  trace_link?: string;
  representative_message?: string;
  representative_stack: string[];
}

export interface ErrorDetailResponse {
  meta: ApmMeta;
  group: ErrorSummary;
  trend: TrendPoint[];
  affected_transactions: TransactionSummary[];
  affected_versions: string[];
  representative_stack: string[];
  samples: ErrorSample[];
}

export interface VersionCompareResponse {
  meta: ApmMeta;
  baseline: { version: string; sample_count: number; red: RedSummary };
  candidate: { version: string; sample_count: number; red: RedSummary };
  sufficient_data: boolean;
  status: 'insufficient_data' | 'regressed' | 'improved' | 'neutral';
  delta: {
    request_count_absolute: number;
    request_count_relative?: number;
    error_rate_absolute: number;
    error_rate_relative?: number;
    p95_absolute_micros?: number;
    p95_relative?: number;
  };
  regressed_transactions: TransactionSummary[];
  regressed_errors: ErrorSummary[];
}

export interface TenantHealthResponse {
  meta: ApmMeta;
  enabled: boolean;
  degraded: boolean;
  runtime?: unknown;
}

export interface ApmQueryParams {
  from?: number;
  to?: number;
  namespace?: string;
  service?: string;
  environment?: string;
  version?: string;
  resolution?: ApmResolution;
  sort?: string;
  direction?: SortDirection;
  limit?: number;
  cursor?: string;
  kind?: TransactionIdentity['kind'];
}

export interface VersionCompareParams extends ApmQueryParams {
  service: string;
  baseline: string;
  candidate: string;
}

function cleanParams(params: ApmQueryParams): Record<string, string | number> {
  return Object.fromEntries(
    Object.entries(params).filter(
      ([, value]) => value !== undefined && value !== null && value !== '',
    ),
  ) as Record<string, string | number>;
}

async function get<T>(path: string, params: ApmQueryParams): Promise<T> {
  const { data } = await http.get<T>(path, { params: cleanParams(params) });
  return data;
}

export const apmApi = {
  overview: (params: ApmQueryParams) => get<OverviewResponse>('/apm/overview', params),
  services: (params: ApmQueryParams) =>
    get<PagedResponse<ServiceSummary>>('/apm/services', params),
  service: (service: string, params: ApmQueryParams) =>
    get<ServiceDetailResponse>(`/apm/services/${encodeURIComponent(service)}`, params),
  transactions: (params: ApmQueryParams) =>
    get<PagedResponse<TransactionSummary>>('/apm/transactions', params),
  transaction: (transaction: string, params: ApmQueryParams) =>
    get<TransactionDetailResponse>(
      `/apm/transactions/${encodeURIComponent(transaction)}`,
      params,
    ),
  dependencies: (params: ApmQueryParams) =>
    get<PagedResponse<DependencySummary>>('/apm/dependencies', params),
  errors: (params: ApmQueryParams) =>
    get<PagedResponse<ErrorSummary>>('/apm/errors', params),
  error: (fingerprint: string, params: ApmQueryParams) =>
    get<ErrorDetailResponse>(`/apm/errors/${encodeURIComponent(fingerprint)}`, params),
  compareVersions: (params: VersionCompareParams) =>
    get<VersionCompareResponse>('/apm/versions/compare', params),
  health: (params: ApmQueryParams) => get<TenantHealthResponse>('/apm/health', params),
};

function stableParams(params: ApmQueryParams) {
  return {
    from: params.from ?? null,
    to: params.to ?? null,
    namespace: params.namespace ?? null,
    service: params.service ?? null,
    environment: params.environment ?? null,
    version: params.version ?? null,
    resolution: params.resolution ?? 'auto',
    sort: params.sort ?? null,
    direction: params.direction ?? 'desc',
    cursor: params.cursor ?? null,
    limit: params.limit ?? null,
    kind: params.kind ?? null,
  };
}

export const apmQueryKeys = {
  all: ['apm'] as const,
  overview: (orgId: string, params: ApmQueryParams) =>
    [...apmQueryKeys.all, 'overview', orgId, stableParams(params)] as const,
  services: (orgId: string, params: ApmQueryParams) =>
    [...apmQueryKeys.all, 'services', orgId, stableParams(params)] as const,
  service: (orgId: string, name: string, params: ApmQueryParams) =>
    [...apmQueryKeys.all, 'service', orgId, name, stableParams(params)] as const,
  transactions: (orgId: string, params: ApmQueryParams) =>
    [...apmQueryKeys.all, 'transactions', orgId, stableParams(params)] as const,
  transaction: (orgId: string, name: string, params: ApmQueryParams) =>
    [...apmQueryKeys.all, 'transaction', orgId, name, stableParams(params)] as const,
  dependencies: (orgId: string, params: ApmQueryParams) =>
    [...apmQueryKeys.all, 'dependencies', orgId, stableParams(params)] as const,
  errors: (orgId: string, params: ApmQueryParams) =>
    [...apmQueryKeys.all, 'errors', orgId, stableParams(params)] as const,
  error: (orgId: string, fingerprint: string, params: ApmQueryParams) =>
    [...apmQueryKeys.all, 'error', orgId, fingerprint, stableParams(params)] as const,
  compare: (orgId: string, params: VersionCompareParams) =>
    [
      ...apmQueryKeys.all,
      'compare',
      orgId,
      stableParams(params),
      params.baseline,
      params.candidate,
    ] as const,
};
