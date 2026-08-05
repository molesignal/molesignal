export function formatCount(value: number): string {
  return new Intl.NumberFormat(undefined, {
    notation: value >= 10_000 ? 'compact' : 'standard',
    maximumFractionDigits: 1,
  }).format(value);
}

export function formatRate(value: number): string {
  return `${(value * 100).toFixed(value >= 0.1 ? 1 : 2)}%`;
}

export function formatThroughput(value?: number): string {
  if (value === undefined || !Number.isFinite(value)) return '—';
  const maximumFractionDigits = value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${new Intl.NumberFormat(undefined, {
    maximumFractionDigits,
  }).format(value)} req/s`;
}

export function formatDuration(micros?: number): string {
  if (micros === undefined) return '—';
  if (micros < 1_000) return `${Math.round(micros)} µs`;
  const millis = micros / 1_000;
  if (millis < 1_000) return `${millis.toFixed(millis < 10 ? 1 : 0)} ms`;
  return `${(millis / 1_000).toFixed(2)} s`;
}

export function formatRelative(value?: number): string {
  if (value === undefined) return '—';
  const sign = value > 0 ? '+' : '';
  return `${sign}${(value * 100).toFixed(1)}%`;
}

export function formatSigned(
  value?: number,
  rate = false,
  duration = false,
): string {
  if (value === undefined) return '—';
  const sign = value > 0 ? '+' : '';
  if (rate) return `${sign}${(value * 100).toFixed(2)}%`;
  if (duration) {
    const formatted = formatDuration(Math.abs(value));
    return value < 0 ? `−${formatted}` : `${sign}${formatted}`;
  }
  return `${sign}${formatCount(value)}`;
}

export function formatTimestamp(micros?: number): string {
  if (micros === undefined) return '—';
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(micros / 1_000));
}

export function statusTone(
  status: 'healthy' | 'warning' | 'critical' | 'no_traffic',
): string {
  if (status === 'healthy') return 'text-green';
  if (status === 'warning') return 'text-yellow';
  if (status === 'critical') return 'text-red';
  return 'text-tx-3';
}
