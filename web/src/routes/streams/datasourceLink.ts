import type { StreamSummary, StreamType } from '@/api/streams';

export type IngestSignal = Exclude<StreamType, 'extend'>;

const DATASOURCE_GUIDE_BY_SIGNAL: Record<IngestSignal, string> = {
  logs: '/datasource/custom/curl',
  metrics: '/datasource/applications/opentelemetry',
  traces: '/datasource/applications/opentelemetry',
  profiles: '/datasource/recommended/continuous-profiling',
};

export function isIngestSignal(value: string | null | undefined): value is IngestSignal {
  return (
    value === 'logs' ||
    value === 'metrics' ||
    value === 'traces' ||
    value === 'profiles'
  );
}

export function ingestPathForSignal(signal: IngestSignal, streamName: string): string {
  if (signal === 'profiles') return '/api/v1/profiles/ingest';
  return `/api/v1/ingest/${signal}/${encodeURIComponent(streamName)}`;
}

export function datasourceLinkForStream(
  stream: Pick<StreamSummary, 'name' | 'stream_type'>,
): string {
  if (!isIngestSignal(stream.stream_type)) return '/datasource';

  const params = new URLSearchParams({
    signal: stream.stream_type,
    stream: stream.name,
  });
  return `${DATASOURCE_GUIDE_BY_SIGNAL[stream.stream_type]}?${params.toString()}`;
}
