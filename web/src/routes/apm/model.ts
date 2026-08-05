import type {
  ApmQueryParams,
  ApmResolution,
  SortDirection,
  TransactionSummary,
} from '@/api/apm';
import type { TraceListSort } from '@/api/web';

export interface ApmUrlFilters {
  namespace: string;
  service: string;
  environment: string;
  version: string;
  search: string;
  category: string;
  resolution: ApmResolution;
  sort: string;
  direction: SortDirection;
  cursor: string;
}

export const DEFAULT_APM_FILTERS: ApmUrlFilters = {
  namespace: '',
  service: '',
  environment: '',
  version: '',
  search: '',
  category: '',
  resolution: 'auto',
  sort: '',
  direction: 'desc',
  cursor: '',
};

export function parseApmFilters(search: string): ApmUrlFilters {
  const params = new URLSearchParams(search);
  const resolution = params.get('resolution');
  const direction = params.get('direction');
  return {
    namespace: params.get('namespace') ?? '',
    service: params.get('service') ?? '',
    environment: params.get('environment') ?? '',
    version: params.get('version') ?? '',
    search: params.get('q') ?? '',
    category: params.get('category') ?? '',
    resolution:
      resolution === 'minute' || resolution === 'hour' ? resolution : 'auto',
    sort: params.get('sort') ?? '',
    direction: direction === 'asc' ? 'asc' : 'desc',
    cursor: params.get('cursor') ?? '',
  };
}

const URL_KEYS: Record<keyof ApmUrlFilters, string> = {
  namespace: 'namespace',
  service: 'service',
  environment: 'environment',
  version: 'version',
  search: 'q',
  category: 'category',
  resolution: 'resolution',
  sort: 'sort',
  direction: 'direction',
  cursor: 'cursor',
};

export function writeApmFilter(
  current: URLSearchParams,
  key: keyof ApmUrlFilters,
  value: string,
): URLSearchParams {
  const next = new URLSearchParams(current);
  const urlKey = URL_KEYS[key];
  const defaultValue = DEFAULT_APM_FILTERS[key];
  if (!value || value === defaultValue) next.delete(urlKey);
  else next.set(urlKey, value);
  if (key !== 'cursor') next.delete('cursor');
  return next;
}

export function apiParamsFromFilters(
  filters: ApmUrlFilters,
  range: { from: Date; to: Date },
  pagination?: { pageSize: number; cursor: string | null },
): ApmQueryParams {
  return {
    from: range.from.getTime() * 1_000,
    to: range.to.getTime() * 1_000,
    resolution: filters.resolution,
    direction: filters.direction,
    limit: pagination?.pageSize ?? 50,
    ...(filters.namespace ? { namespace: filters.namespace } : {}),
    ...(filters.service ? { service: filters.service } : {}),
    ...(filters.environment ? { environment: filters.environment } : {}),
    ...(filters.version ? { version: filters.version } : {}),
    ...(filters.sort ? { sort: filters.sort } : {}),
    ...(pagination
      ? pagination.cursor
        ? { cursor: pagination.cursor }
        : {}
      : filters.cursor
        ? { cursor: filters.cursor }
        : {}),
  };
}

export function hasActiveEntityFilters(filters: ApmUrlFilters): boolean {
  return Boolean(
    filters.namespace ||
      filters.service ||
      filters.environment ||
      filters.version ||
      filters.search ||
      filters.category,
  );
}

export function servicePath(
  service: { namespace: string; name: string; environment: string },
  suffix = '',
): string {
  const params = new URLSearchParams({
    namespace: service.namespace,
    environment: service.environment,
  });
  return `/apm/services/${encodeURIComponent(service.name)}${suffix}?${params}`;
}

export function transactionPath(transaction: TransactionSummary): string {
  const params = new URLSearchParams({
    namespace: transaction.service.namespace,
    service: transaction.service.name,
    environment: transaction.service.environment,
    kind: transaction.transaction.kind,
  });
  if (transaction.version) params.set('version', transaction.version);
  return `/apm/transactions/${encodeURIComponent(transaction.transaction.name)}?${params}`;
}

export function signalHref(
  target: 'traces' | 'logs' | 'metrics' | 'profiles',
  handle: {
    service: string;
    namespace: string;
    environment: string;
    version?: string;
    transaction?: string;
    error_fingerprint?: string;
    from: number;
    to: number;
  },
  options: { traceSort?: TraceListSort } = {},
): string {
  const params = new URLSearchParams({
    service: handle.service,
    namespace: handle.namespace,
    environment: handle.environment,
    from: new Date(handle.from / 1_000).toISOString(),
    to: new Date(handle.to / 1_000).toISOString(),
  });
  if (handle.version) params.set('version', handle.version);
  if (handle.transaction) params.set('transaction', handle.transaction);
  if (handle.error_fingerprint) params.set('error', handle.error_fingerprint);
  if (target === 'traces' && options.traceSort) {
    params.set('sort', options.traceSort);
  }
  return `/${target}?${params}`;
}
