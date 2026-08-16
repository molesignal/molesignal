import type {
  StreamRuntime,
  StreamRuntimeStatus,
  StreamType,
} from '@/api/streams';

export type UnifiedStreamHealthStatus =
  | 'healthy'
  | 'degraded'
  | 'delayed'
  | 'no_data'
  | 'unknown';

export type StreamHealthFilter = 'all' | 'attention' | StreamRuntimeStatus;

export interface StreamHealthSummary {
  total: number;
  receiving: number;
  attention: number;
  inactive: number;
  status: UnifiedStreamHealthStatus;
}

export function isAttentionRuntimeStatus(status: StreamRuntimeStatus): boolean {
  return status === 'delayed' || status === 'interrupted' || status === 'unknown';
}

export function matchesStreamHealthFilter(
  status: StreamRuntimeStatus,
  filter: StreamHealthFilter,
): boolean {
  if (filter === 'all') return true;
  if (filter === 'attention') return isAttentionRuntimeStatus(status);
  return status === filter;
}

export function runtimeStatusToHealthStatus(
  status: StreamRuntimeStatus,
): UnifiedStreamHealthStatus {
  if (status === 'healthy') return 'healthy';
  if (status === 'delayed') return 'delayed';
  if (status === 'interrupted') return 'degraded';
  if (status === 'unknown') return 'unknown';
  return 'no_data';
}

export function summarizeStreamHealth(
  streams: Array<Pick<StreamRuntime, 'status'>>,
): StreamHealthSummary {
  const receiving = streams.filter((stream) => stream.status === 'healthy').length;
  const attention = streams.filter((stream) =>
    isAttentionRuntimeStatus(stream.status),
  ).length;
  const inactive = streams.length - receiving - attention;
  const statuses = streams.map((stream) => stream.status);
  const status: UnifiedStreamHealthStatus = statuses.some(
    (value) => value === 'interrupted',
  )
    ? 'degraded'
    : statuses.some((value) => value === 'unknown')
      ? 'unknown'
      : statuses.some((value) => value === 'delayed')
        ? 'delayed'
        : receiving > 0
          ? 'healthy'
          : 'no_data';
  return { total: streams.length, receiving, attention, inactive, status };
}

export function summarizeSignalHealth(
  streams: StreamRuntime[],
  streamType: Exclude<StreamType, 'extend'>,
): StreamHealthSummary & {
  rows: number;
  lastReceivedAtMicros: number | null;
} {
  const matching = streams.filter((stream) => stream.stream_type === streamType);
  const summary = summarizeStreamHealth(matching);
  return {
    ...summary,
    rows: matching.reduce((total, stream) => total + stream.rows, 0),
    lastReceivedAtMicros:
      matching.reduce<number | null>((latest, stream) => {
        const received = stream.last_received_at_micros;
        if (received == null) return latest;
        return latest == null ? received : Math.max(latest, received);
      }, null),
  };
}

export function streamHealthFilterFromParams(
  params: URLSearchParams,
): StreamHealthFilter {
  const value = params.get('status');
  return isStreamHealthFilter(value) ? value : 'all';
}

export function streamHealthWindowFromParams(
  params: URLSearchParams,
  fallback = 24 * 60 * 60,
): number {
  const value = Number(params.get('window_secs'));
  return Number.isFinite(value) && value >= 15 * 60 && value <= 7 * 24 * 60 * 60
    ? Math.round(value)
    : fallback;
}

export function streamAttentionHref(windowSecs: number): string {
  const params = new URLSearchParams({
    status: 'attention',
    window_secs: String(windowSecs),
  });
  const from = relativeWindowExpression(windowSecs);
  params.set('time', `${from}..now`);
  return `/streams?${params}`;
}

function isStreamHealthFilter(value: string | null): value is StreamHealthFilter {
  return [
    'all',
    'attention',
    'healthy',
    'idle',
    'delayed',
    'interrupted',
    'unused',
    'unknown',
  ].includes(value ?? '');
}

function relativeWindowExpression(windowSecs: number): string {
  if (windowSecs % 86_400 === 0) return `now-${windowSecs / 86_400}d`;
  if (windowSecs % 3_600 === 0) return `now-${windowSecs / 3_600}h`;
  if (windowSecs % 60 === 0) return `now-${windowSecs / 60}m`;
  return `now-${windowSecs}s`;
}
