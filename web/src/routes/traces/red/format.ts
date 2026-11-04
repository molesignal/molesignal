import { formatTraceDurationNs } from '@/viz/trace/duration';

import type { RateUnit } from './model';

const RATE_SUFFIX: Record<RateUnit['key'], string> = {
  per_second: 'req/s',
  per_minute: 'req/min',
  per_hour: 'req/h',
};

/** A count, exact under ten thousand and compact above. */
export function formatRequests(count: number, locale?: string): string {
  return new Intl.NumberFormat(locale, {
    notation: count >= 10_000 ? 'compact' : 'standard',
    maximumFractionDigits: 1,
  }).format(count);
}

/** A rate in `unit`, with as many decimals as keep it readable. */
export function formatRate(perSecond: number | null, unit: RateUnit, locale?: string): string {
  if (perSecond === null || !Number.isFinite(perSecond)) return '—';

  const value = perSecond * unit.seconds;
  const maximumFractionDigits = value >= 100 ? 0 : value >= 10 ? 1 : 2;

  return `${new Intl.NumberFormat(locale, { maximumFractionDigits }).format(value)} ${RATE_SUFFIX[unit.key]}`;
}

/** The unit a rate chart's axis is read in. */
export function rateSuffix(unit: RateUnit): string {
  return RATE_SUFFIX[unit.key];
}

/** A share that failed, as a percentage. */
export function formatErrorRate(rate: number | null): string {
  if (rate === null || !Number.isFinite(rate)) return '—';
  if (rate === 0) return '0%';

  return `${(rate * 100).toFixed(rate >= 0.1 ? 1 : 2)}%`;
}

/** A duration as a reader takes it in: a second and longer in seconds, shorter as a trace says it. */
export function formatDuration(ns: number | null | undefined): string {
  if (ns === null || ns === undefined || !Number.isFinite(ns)) return '—';
  if (Math.abs(ns) >= 1e9) return `${Number((ns / 1e9).toFixed(ns >= 1e10 ? 1 : 2))} s`;

  return formatTraceDurationNs(ns);
}

/** An edge of the duration axis, as short as it reads: 1 µs, 10 ms, 1 s. */
export function formatDurationEdge(ns: number): string {
  if (ns >= 1e9) return `${trim(ns / 1e9)} s`;
  if (ns >= 1e6) return `${trim(ns / 1e6)} ms`;
  if (ns >= 1e3) return `${trim(ns / 1e3)} µs`;

  return `${trim(ns)} ns`;
}

function trim(value: number): string {
  return Number(value.toPrecision(3)).toString();
}
