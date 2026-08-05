function compactFixed(value: number, fractionDigits: number): string {
  return Number(value.toFixed(fractionDigits)).toString();
}

/**
 * Formats trace timings with just enough precision to remain useful.
 *
 * Trace durations are stored as nanoseconds, but the waterfall is primarily
 * read in milliseconds. Sub-millisecond values switch to microseconds so the
 * UI never fills up with meaningless `0.00 ms` labels.
 */
export function formatTraceDurationNs(ns: number): string {
  if (!Number.isFinite(ns)) return '—';

  const sign = ns < 0 ? '-' : '';
  const absoluteNs = Math.abs(ns);
  const milliseconds = absoluteNs / 1_000_000;

  if (milliseconds >= 100) {
    return `${sign}${Math.round(milliseconds)} ms`;
  }
  if (milliseconds >= 10) {
    return `${sign}${compactFixed(milliseconds, 1)} ms`;
  }
  if (milliseconds >= 1) {
    return `${sign}${compactFixed(milliseconds, 2)} ms`;
  }

  const microseconds = absoluteNs / 1_000;
  const fractionDigits = microseconds >= 100 ? 0 : microseconds >= 10 ? 1 : 2;
  return `${sign}${compactFixed(microseconds, fractionDigits)} μs`;
}

export function formatTraceDurationMs(milliseconds: number): string {
  return formatTraceDurationNs(milliseconds * 1_000_000);
}
