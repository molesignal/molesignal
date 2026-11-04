import { http } from '@/lib/http';

import type { ServiceDefinition, ServiceDefinitionWrite } from './apmCatalog';
import type { ErrorBreakdown, ErrorIssue, ErrorIssueFilter, ErrorIssueStatus } from './apmErrors';
import type { ServiceInstance, ServiceProfile } from './apmProfile';

export type ApmResolution = 'auto' | 'minute' | 'hour';
export type SortDirection = 'asc' | 'desc';

export interface TimeRange {
  from: number;
  to: number;
}

export interface ProjectionGap {
  orgId: string;
  range: { start: number; end: number };
  reason:
    | 'queue_full'
    | 'repository_unavailable'
    | 'flush_failed'
    | 'late_dropped'
    | 'cardinality_rejected'
    | 'shutdown_timeout';
  droppedFacts: number;
  recordedAt: number;
}

export interface DataQuality {
  partial: boolean;
  gaps: ProjectionGap[];
  overflowDimensions: string[];
}

export interface ApmMeta {
  range: TimeRange;
  resolution: Exclude<ApmResolution, 'auto'>;
  projectionStartedAt?: number;
  lastCompleteBucketAt?: number;
  dataQuality: DataQuality;
  activationBoundary: boolean;
}

export interface TraceExemplar {
  traceId: string;
  spanId: string;
  eventTime: number;
  duration: number;
  traceAvailable: boolean;
}

export interface RedSummary {
  requestCount: number;
  errorCount: number;
  errorRate: number;
  durationSum: number;
  durationAverage?: number;
  p50Time?: number;
  /** Older backends answer without it. */
  p90Time?: number | null;
  p95Time?: number;
  p99Time?: number;
  /** Satisfied plus half the tolerating requests over all of them; a failed request is never satisfied. */
  apdex?: number | null;
  latencyPartial: boolean;
  exemplars: TraceExemplar[];
  /** The requests by what they answered (`http:404`, `grpc:14`); absent before status codes were counted. */
  statusCodes?: Record<string, number>;
  /** Requests that ran out of time, and requests sent again; absent when none. */
  timeoutCount?: number;
  retryCount?: number;
}

export type ServiceHealth = 'healthy' | 'warning' | 'critical' | 'no_traffic';

/** A figure of a service past the line that makes it a warning or critical. */
export interface HealthReason {
  metric: 'error_rate' | 'p95';
  level: ServiceHealth;
  value: number;
  threshold: number;
}

export interface TrendPoint {
  bucketAt: number;
  red: RedSummary;
}

/** A bucket of the trend a list draws beside a row. */
export interface SparkPoint {
  bucketAt: number;
  requestCount: number;
  errorCount: number;
  p95Time?: number | null;
}

/** A bucket of a latency histogram that holds requests, narrowed to the fastest and slowest of them. */
export interface LatencyBucket {
  lowerTime: number;
  upperTime: number;
  count: number;
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
  errorFingerprint?: string;
  from: number;
  to: number;
}

export interface ServiceSummary {
  service: ServiceIdentity;
  firstSeenAt: number;
  lastSeenAt: number;
  instrumentation: {
    runtimeLanguage?: string;
    telemetrySdkName?: string;
    telemetrySdkVersion?: string;
    recentInstanceCount: number;
  };
  versions: string[];
  /** The version seen most recently, and when it was first seen: the last deploy. */
  currentVersion?: string | null;
  currentVersionSince?: number | null;
  red: RedSummary;
  /** The Apdex threshold T the summary was read with. Older backends answer without it. */
  apdexThreshold?: number;
  /** Older backends answer without it. */
  sparkline?: SparkPoint[];
  health: ServiceHealth;
  /** What made the health what it is. Older backends answer without it. */
  healthReasons?: HealthReason[];
  traces: SignalFilterHandle;
  /** What the service runs as within the range; the catalog and a service's page carry it. */
  profile?: ServiceProfile | null;
}

export type { ProfileValue, ServiceInstance, ServiceProfile } from './apmProfile';

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
  errorType: string;
  applicationFrame?: string;
  transactionName?: string;
  overflow: boolean;
}

export interface TransactionSummary {
  service: ServiceIdentity;
  version?: string;
  transaction: TransactionIdentity;
  red: RedSummary;
  /** The Apdex threshold T of the endpoint's service. Older backends answer without it. */
  apdexThreshold?: number;
  totalTime: number;
  traces: SignalFilterHandle;
}

export interface DependencySummary {
  service: ServiceIdentity;
  version?: string;
  dependency: DependencyIdentity;
  red: RedSummary;
  totalTime: number;
  traces: SignalFilterHandle;
}

export interface ErrorSummary {
  error: ErrorIdentity;
  service: ServiceIdentity;
  firstSeenAt: number;
  lastSeenAt: number;
  occurrenceCount: number;
  representativeMessage?: string;
  red: RedSummary;
  traces: SignalFilterHandle;
  /** `null` while nobody has touched the group: it reads as open. */
  issue?: ErrorIssue | null;
}

export interface VersionSummary {
  service: ServiceIdentity;
  version: string;
  firstSeenAt: number;
  lastSeenAt: number;
  observationCount: number;
  /** What the version served in the time in view, where it was read. */
  red?: RedSummary;
  /** The version against the one of its service first seen before it. */
  verdict?: VersionVerdict;
}

export type VersionStatus = 'insufficient_data' | 'regressed' | 'improved' | 'neutral';

export interface VersionDelta {
  requestCountAbsolute: number;
  requestCountRelative?: number;
  errorRateAbsolute: number;
  errorRateRelative?: number;
  p95AbsoluteTime?: number;
  p95Relative?: number;
}

export interface VersionVerdict {
  /** The version it is told against. */
  previous: string;
  status: VersionStatus;
  /** What got worse. */
  worse?: ('error_rate' | 'latency')[];
  delta: VersionDelta;
}

export interface PagedResponse<T> {
  meta: ApmMeta;
  items: T[];
  nextCursor: string | null;
  previousCursor: string | null;
  hasMore: boolean;
  sort: string;
}

export interface OverviewResponse {
  meta: ApmMeta;
  red: RedSummary;
  trend: TrendPoint[];
  serviceHealth: {
    healthy: number;
    warning: number;
    critical: number;
    no_traffic: number;
  };
  services: ServiceSummary[];
  topTransactions: TransactionSummary[];
  topDependencies: DependencySummary[];
  topErrors: ErrorSummary[];
  recentVersions: VersionSummary[];
  /** Services seen in the range, and those of them that served any request. */
  serviceCount?: number;
  activeServiceCount?: number;
  healthTrend?: HealthTrendPoint[];
  rankings?: ServiceRankings;
  /** Versions first seen within the range, the newest first. */
  deployments?: VersionSummary[];
  environments?: EnvironmentShare[];
  /** Every environment the services run in, whichever one is in view. */
  availableEnvironments?: string[];
  instrumentation?: InstrumentationQuality;
}

export interface ServiceDetailResponse {
  meta: ApmMeta;
  service: ServiceSummary;
  red: RedSummary;
  trend: TrendPoint[];
  latencyDistribution?: LatencyBucket[];
  transactions: TransactionSummary[];
  dependencies: DependencySummary[];
  errors: ErrorSummary[];
  versions: VersionSummary[];
}

export interface TransactionDetailResponse {
  meta: ApmMeta;
  transaction: TransactionSummary;
  trend: TrendPoint[];
  latencyDistribution?: LatencyBucket[];
  errors: ErrorSummary[];
  versions: VersionSummary[];
}

export interface ErrorSample {
  eventTime: number;
  traceId: string;
  spanId: string;
  traceAvailable: boolean;
  traceLink?: string;
  representativeMessage?: string;
  representativeStack: string[];
}

export interface ErrorDetailResponse {
  meta: ApmMeta;
  group: ErrorSummary;
  trend: TrendPoint[];
  affectedTransactions: TransactionSummary[];
  affectedVersions: string[];
  /** The group's occurrences by version, environment and language. */
  breakdown?: ErrorBreakdown;
  representativeStack: string[];
  samples: ErrorSample[];
}

/** A page of error groups, with the occurrences of every group the list keeps by version, environment and language. */
export interface ErrorListResponse extends PagedResponse<ErrorSummary> {
  breakdown?: ErrorBreakdown;
}

export interface VersionCompareResponse {
  meta: ApmMeta;
  baseline: { version: string; sampleCount: number; red: RedSummary };
  candidate: { version: string; sampleCount: number; red: RedSummary };
  sufficientData: boolean;
  status: VersionStatus;
  delta: VersionDelta;
  regressedTransactions: TransactionSummary[];
  regressedErrors: ErrorSummary[];
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
  status?: ErrorIssueFilter;
}

export type {
  CoverageState,
  ServiceCoverage,
  ServiceDefinition,
  ServiceDefinitionWrite,
  ServiceLifecycle,
  ServiceTier,
  SignalCoverage,
} from './apmCatalog';

export type { ErrorRule, ErrorRuleAction, ErrorRuleWrite } from './apmErrorRules';
export type { ErrorBreakdown, ErrorIssue, ErrorIssueFilter, ErrorIssueStatus, ErrorShare } from './apmErrors';

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
  instances: (service: string, params: ApmQueryParams) =>
    get<ServiceInstance[]>(`/apm/services/${encodeURIComponent(service)}/instances`, params),
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
    get<ErrorListResponse>('/apm/errors', params),
  error: (fingerprint: string, params: ApmQueryParams) =>
    get<ErrorDetailResponse>(`/apm/errors/${encodeURIComponent(fingerprint)}`, params),
  compareVersions: (params: VersionCompareParams) =>
    get<VersionCompareResponse>('/apm/versions/compare', params),
  health: (params: ApmQueryParams) => get<TenantHealthResponse>('/apm/health', params),
  definitions: async () => {
    const { data } = await http.get<ServiceDefinition[]>('/apm/service-definitions');
    return data;
  },
  saveDefinition: async (service: { namespace: string; name: string }, write: ServiceDefinitionWrite) => {
    const { data } = await http.put<ServiceDefinition>(
      `/apm/service-definitions/${encodeURIComponent(service.name)}`,
      write,
      { params: { namespace: service.namespace } },
    );
    return data;
  },
  deleteDefinition: async (service: { namespace: string; name: string }) => {
    await http.delete(`/apm/service-definitions/${encodeURIComponent(service.name)}`, {
      params: { namespace: service.namespace },
    });
  },
  saveIssue: async (
    fingerprint: string,
    write: { status: ErrorIssueStatus; assigneeId?: string | null; teamId?: string | null; lastSeenAt?: number },
  ) => {
    const { data } = await http.put<ErrorIssue>(`/apm/errors/${encodeURIComponent(fingerprint)}/issue`, write);
    return data;
  },
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
    status: params.status ?? null,
  };
}

export const apmQueryKeys = {
  all: ['apm'] as const,
  definitions: (orgId: string) => [...apmQueryKeys.all, 'definitions', orgId] as const,
  overview: (orgId: string, params: ApmQueryParams) =>
    [...apmQueryKeys.all, 'overview', orgId, stableParams(params)] as const,
  services: (orgId: string, params: ApmQueryParams) =>
    [...apmQueryKeys.all, 'services', orgId, stableParams(params)] as const,
  service: (orgId: string, name: string, params: ApmQueryParams) =>
    [...apmQueryKeys.all, 'service', orgId, name, stableParams(params)] as const,
  instances: (orgId: string, name: string, params: ApmQueryParams) =>
    [...apmQueryKeys.all, 'instances', orgId, name, stableParams(params)] as const,
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

export interface ServiceHealthCounts {
  healthy: number;
  warning: number;
  critical: number;
  no_traffic: number;
}

export interface HealthTrendPoint {
  bucketAt: number;
  counts: ServiceHealthCounts;
}

/** A service as a ranking lists it. */
export interface ServiceRank {
  service: ServiceIdentity;
  health: ServiceHealth;
  requestCount: number;
  errorRate: number;
  p95Time?: number | null;
}

export interface ServiceRankings {
  slowest: ServiceRank[];
  most_errors: ServiceRank[];
  busiest: ServiceRank[];
}

export interface EnvironmentShare {
  environment: string;
  serviceCount: number;
  requestCount: number;
}

/** What the services report about how they are instrumented, and what they leave out. */
export interface InstrumentationQuality {
  sdks: Array<{
    language?: string | null;
    sdkName?: string | null;
    sdkVersion?: string | null;
    serviceCount: number;
  }>;
  withoutEnvironment: number;
  withoutVersion: number;
  withoutInstance: number;
}
