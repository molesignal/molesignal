import type { ApmQueryParams, ApmResolution, ErrorIssueFilter, ServiceIdentity } from '@/api/apm';

/**
 * What is drawn under the current figures: the same stretch of time a day or
 * a week earlier, or the same stretch in another environment of the service
 * (`env:<name>`).
 */
export type ComparePeriod = 'off' | 'day' | 'week' | `env:${string}`;

/** What an APM page is looking at, as its address says it. */
export interface ApmScope {
  /** Every environment while empty. */
  environment: string;
  /** A service page: one version of the service, or every one while empty. */
  version: string;
  /** A list: what names are searched for. */
  search: string;
  resolution: ApmResolution;
  /** An error list: which groups it keeps by what is being done about them. */
  status: ErrorIssueFilter;
  compare: ComparePeriod;
}

export const DEFAULT_APM_SCOPE: ApmScope = {
  environment: '',
  version: '',
  search: '',
  resolution: 'auto',
  status: 'unresolved',
  compare: 'off',
};

const URL_KEYS: Record<keyof ApmScope, string> = {
  environment: 'environment',
  version: 'version',
  search: 'q',
  resolution: 'resolution',
  status: 'status',
  compare: 'compare',
};

const STATUSES: readonly ErrorIssueFilter[] = ['all', 'unresolved', 'investigating', 'resolved', 'ignored'];

export function parseApmScope(search: string | URLSearchParams): ApmScope {
  const params = typeof search === 'string' ? new URLSearchParams(search) : search;
  const resolution = params.get(URL_KEYS.resolution);
  const status = params.get(URL_KEYS.status) as ErrorIssueFilter | null;
  const compare = params.get(URL_KEYS.compare);

  return {
    environment: params.get(URL_KEYS.environment) ?? '',
    version: params.get(URL_KEYS.version) ?? '',
    search: params.get(URL_KEYS.search) ?? '',
    resolution: resolution === 'minute' || resolution === 'hour' ? resolution : 'auto',
    status: status && STATUSES.includes(status) ? status : DEFAULT_APM_SCOPE.status,
    compare: compare === 'day' || compare === 'week' || (compare?.startsWith('env:') && compare.length > 4) ? (compare as ComparePeriod) : 'off',
  };
}

/** The address with `patch` written into it; a value equal to its default leaves the address. */
export function writeApmScope(current: URLSearchParams, patch: Partial<ApmScope>): URLSearchParams {
  const next = new URLSearchParams(current);

  for (const [key, value] of Object.entries(patch) as Array<[keyof ApmScope, string]>) {
    if (!value || value === DEFAULT_APM_SCOPE[key]) next.delete(URL_KEYS[key]);
    else next.set(URL_KEYS[key], value);
  }

  // A page of a list counted under other filters is not a page of this one.
  next.delete('cursor');
  return next;
}

/** What the APM API is asked with for the time in view. */
export function apmQueryParams(scope: ApmScope, range: { from: Date; to: Date }, extra: ApmQueryParams = {}): ApmQueryParams {
  return {
    from: range.from.getTime() * 1_000,
    to: range.to.getTime() * 1_000,
    resolution: scope.resolution,
    ...(scope.environment ? { environment: scope.environment } : {}),
    ...extra,
  };
}

/** The environment a comparison is with, when it is with one. */
export function compareEnvironment(period: ComparePeriod): string | null {
  return period.startsWith('env:') ? period.slice(4) : null;
}

/** How many microseconds a comparison period sets the time back. */
export function compareShiftMicros(period: ComparePeriod): number {
  if (period === 'day') return 86_400_000_000;
  if (period === 'week') return 7 * 86_400_000_000;
  return 0;
}

export const SERVICE_SECTIONS = ['overview', 'endpoints', 'traces', 'dependencies', 'errors', 'deployments', 'runtime', 'settings'] as const;

export type ServiceSection = (typeof SERVICE_SECTIONS)[number];

export function serviceSectionOf(raw: string | undefined): ServiceSection | null {
  if (raw === undefined) return 'overview';
  return (SERVICE_SECTIONS as readonly string[]).includes(raw) ? (raw as ServiceSection) : null;
}

/** What a service page keeps of the address when the reader moves between its sections. */
const CARRIED = ['version', 'resolution', 'compare'] as const;

/**
 * A section of a service's page. The namespace and the environment are part
 * of which service it is; the version, resolution and comparison of the page
 * the reader is on go with them.
 */
export function servicePath(
  service: Pick<ServiceIdentity, 'namespace' | 'name' | 'environment'>,
  section: ServiceSection = 'overview',
  carry?: URLSearchParams,
): string {
  const params = new URLSearchParams({
    namespace: service.namespace,
    environment: service.environment,
  });

  for (const key of CARRIED) {
    const value = carry?.get(key);
    if (value) params.set(key, value);
  }

  const suffix = section === 'overview' ? '' : `/${section}`;
  return `/apm/services/${encodeURIComponent(service.name)}${suffix}?${params}`;
}

/** A service across its environments: what its definition is keyed by. */
export function serviceKey(service: { namespace: string; name: string }): string {
  return `${service.namespace}/${service.name}`;
}

/** The overflow row the projection folds names past its limits into. */
export const OTHER_DIMENSION = '__other__';
