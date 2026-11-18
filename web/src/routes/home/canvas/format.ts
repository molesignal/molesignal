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

export function formatCount(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  const abs = Math.abs(value);
  if (abs < 1_000) return `${Math.round(value)}`;
  if (abs < 1_000_000) return `${(value / 1_000).toFixed(1)}K`;
  if (abs < 1_000_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  return `${(value / 1_000_000_000).toFixed(2)}B`;
}
