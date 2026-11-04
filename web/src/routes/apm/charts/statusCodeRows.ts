import type { TrendPoint } from '@/api/apm';
import type { TimeSeriesSeries } from '@/viz/timeseries/types';

/** gRPC's codes by number, as the protocol names them. */
const GRPC_CODES = [
  'OK',
  'CANCELLED',
  'UNKNOWN',
  'INVALID_ARGUMENT',
  'DEADLINE_EXCEEDED',
  'NOT_FOUND',
  'ALREADY_EXISTS',
  'PERMISSION_DENIED',
  'RESOURCE_EXHAUSTED',
  'FAILED_PRECONDITION',
  'ABORTED',
  'OUT_OF_RANGE',
  'UNIMPLEMENTED',
  'INTERNAL',
  'UNAVAILABLE',
  'DATA_LOSS',
  'UNAUTHENTICATED',
] as const;

export type StatusClass = '1xx' | '2xx' | '3xx' | '4xx' | '5xx' | 'grpc_ok' | 'grpc_error' | 'other';

/** The order classes are stacked and listed in, the healthy at the bottom. */
export const STATUS_CLASSES: readonly StatusClass[] = ['2xx', 'grpc_ok', '1xx', '3xx', '4xx', 'grpc_error', '5xx', 'other'];

/** Each class in the ink of what it says: fine, moved, refused, failed. */
export const STATUS_TEXT: Record<StatusClass, string> = {
  '1xx': 'text-tx-1',
  '2xx': 'text-[var(--green-ink)]',
  '3xx': 'text-[var(--yellow-ink)]',
  '4xx': 'text-[var(--orange-ink)]',
  '5xx': 'text-[var(--red-ink)]',
  grpc_ok: 'text-[var(--green-ink)]',
  grpc_error: 'text-[var(--red-ink)]',
  other: 'text-tx-2',
};

export const STATUS_FILL: Record<StatusClass, string> = {
  '1xx': 'var(--chart-1)',
  '2xx': 'var(--green)',
  '3xx': 'var(--yellow)',
  '4xx': 'var(--orange)',
  '5xx': 'var(--red)',
  grpc_ok: 'var(--green)',
  grpc_error: 'var(--red)',
  other: 'var(--tx-4)',
};

export interface StatusOf {
  protocol: 'http' | 'grpc' | 'other';
  /** The code as people say it: `404`, `UNAVAILABLE`; empty for the rest of a protocol. */
  label: string;
  /** The number behind a gRPC name. */
  code: number | null;
  statusClass: StatusClass;
}

/** What a counted status is: `http:404`, `grpc:14`, `http:other`. */
export function statusOf(key: string): StatusOf {
  const [protocol = '', value = ''] = key.split(':');
  const code = /^\d+$/.test(value) ? Number(value) : null;

  if (protocol === 'http' && code !== null) {
    return { protocol, label: String(code), code, statusClass: `${Math.floor(code / 100)}xx` as StatusClass };
  }

  if (protocol === 'grpc' && code !== null) {
    return { protocol, label: GRPC_CODES[code] ?? String(code), code, statusClass: code === 0 ? 'grpc_ok' : 'grpc_error' };
  }

  return { protocol: protocol === 'http' || protocol === 'grpc' ? protocol : 'other', label: '', code: null, statusClass: 'other' };
}

export interface StatusRow extends StatusOf {
  key: string;
  count: number;
  share: number;
}

/** The statuses the requests answered, the commonest first. */
export function statusRows(codes: Readonly<Record<string, number>> | undefined): StatusRow[] {
  const entries = Object.entries(codes ?? {}).filter(([, count]) => count > 0);
  const total = entries.reduce((sum, [, count]) => sum + count, 0);

  return entries
    .map(([key, count]) => ({ key, count, share: total > 0 ? count / total : 0, ...statusOf(key) }))
    .sort((left, right) => right.count - left.count || left.key.localeCompare(right.key));
}

/** The requests a second of each class, bucket by bucket, stacked from the healthy up. */
export function statusClassSeries(trend: readonly TrendPoint[], seconds: number, label: (statusClass: StatusClass) => string): TimeSeriesSeries[] {
  const timestamps = trend.map((point) => point.bucketAt);
  const perPoint = trend.map((point) => {
    const counts = new Map<StatusClass, number>();
    for (const [key, count] of Object.entries(point.red.statusCodes ?? {})) {
      const statusClass = statusOf(key).statusClass;
      counts.set(statusClass, (counts.get(statusClass) ?? 0) + count);
    }
    return counts;
  });

  return STATUS_CLASSES.filter((statusClass) => perPoint.some((counts) => counts.has(statusClass))).map((statusClass) => ({
    id: statusClass,
    name: label(statusClass),
    timestamps,
    data: perPoint.map((counts) => (counts.get(statusClass) ?? 0) / seconds),
    color: STATUS_FILL[statusClass],
    unit: 'req/s',
  }));
}
