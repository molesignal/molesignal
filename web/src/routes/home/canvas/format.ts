import type { AuditEvent } from '@/api/audit';
import type { HomeStreamOverview } from '@/api/home';

/** Humanized "firing for" age from a microsecond epoch. */
export function formatAge(createdMicros: number | undefined): string {
  if (!createdMicros) return '—';
  const ms = Date.now() - Math.floor(createdMicros / 1000);
  if (ms <= 0) return '0m';
  const mins = Math.floor(ms / 60_000);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ${mins % 60}m`;
  return `${Math.floor(hrs / 24)}d ${hrs % 24}h`;
}

export function formatBytes(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  const abs = Math.abs(value);
  if (abs < 1024) return `${Math.round(value)} B`;
  if (abs < 1024 ** 2) return `${(value / 1024).toFixed(1)} KiB`;
  if (abs < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MiB`;
  if (abs < 1024 ** 4) return `${(value / 1024 ** 3).toFixed(2)} GiB`;
  return `${(value / 1024 ** 4).toFixed(2)} TiB`;
}

export function formatBytesCompact(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  const abs = Math.abs(value);
  if (abs < 1024) return `${Math.round(value)} B`;
  if (abs < 1024 ** 2) return `${Math.round(value / 1024)} KiB`;
  if (abs < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MiB`;
  if (abs < 1024 ** 4) return `${(value / 1024 ** 3).toFixed(1)} GiB`;
  return `${(value / 1024 ** 4).toFixed(1)} TiB`;
}

export function formatCount(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  const abs = Math.abs(value);
  if (abs < 1_000) return `${Math.round(value)}`;
  if (abs < 1_000_000) return `${(value / 1_000).toFixed(1)}K`;
  if (abs < 1_000_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  return `${(value / 1_000_000_000).toFixed(2)}B`;
}

export function formatEventRate(rows: number, windowSecs: number): string {
  if (rows <= 0 || windowSecs <= 0) return '—';
  const perSecond = rows / windowSecs;
  if (perSecond >= 1) return `${formatCount(perSecond)}/s`;
  const perMinute = perSecond * 60;
  if (perMinute >= 1) return `${formatCount(perMinute)}/min`;
  return `${formatCount(perMinute * 60)}/h`;
}

export function formatByteRate(
  bytes: number | null | undefined,
  windowSecs: number,
): string {
  if (bytes == null || bytes <= 0 || windowSecs <= 0) return '—';
  return `${formatBytes(bytes / (windowSecs / 3600))}/h`;
}

export function streamExplorePath(stream: HomeStreamOverview): string {
  const name = encodeURIComponent(stream.name);
  if (stream.stream_type === 'logs') return `/logs?stream=${name}`;
  if (stream.stream_type === 'metrics') return `/metrics?metric=${name}`;
  if (stream.stream_type === 'traces') return `/traces?stream=${name}`;
  return `/streams/${encodeURIComponent(stream.id)}`;
}

export function auditTarget(event: AuditEvent): string {
  for (const key of ['name', 'title', 'summary', 'email']) {
    const value = event.payload[key];
    if (typeof value === 'string' && value.trim()) return value;
  }
  return event.target_id ?? event.target_kind ?? event.actor_kind;
}

export function humanizeAction(action: string): string {
  return action
    .replace(/[._:/-]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}
