import type { StreamSummary, StreamType } from '@/api/streams';

export type IntakeSignal = Exclude<StreamType, 'extend'>;

const DATASOURCE_GUIDE_BY_SIGNAL: Record<IntakeSignal, string> = {
  logs: '/datasource/custom/curl',
  metrics: '/datasource/applications/opentelemetry',
  traces: '/datasource/applications/opentelemetry',
  profiles: '/datasource/recommended/continuous-profiling',
};

export function isIntakeSignal(value: string | null | undefined): value is IntakeSignal {
  return (
    value === 'logs' ||
    value === 'metrics' ||
    value === 'traces' ||
    value === 'profiles'
  );
}

export function intakePathForSignal(signal: IntakeSignal, streamName: string): string {
  if (signal === 'profiles') return '/api/v1/profiles/intake';
  return `/api/v1/intake/${signal}/${encodeURIComponent(streamName)}`;
}

export function datasourceLinkForStream(
  stream: Pick<StreamSummary, 'name' | 'stream_type'>,
): string {
  if (!isIntakeSignal(stream.stream_type)) return '/datasource';

  const params = new URLSearchParams({
    signal: stream.stream_type,
    stream: stream.name,
  });
  return `${DATASOURCE_GUIDE_BY_SIGNAL[stream.stream_type]}?${params.toString()}`;
}
