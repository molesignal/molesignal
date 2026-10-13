/**
 * How often a scheduled pipeline runs and how far back each run reads.
 *
 * The backend stores the interval as `every:<n><s|m|h>` (the field is still
 * called `cron`) and the lookback as seconds. People think in "every 5
 * minutes", so the form holds a count and a unit and these turn one into the
 * other.
 */
export type IntervalUnit = 's' | 'm' | 'h';

export interface Interval {
  count: number;
  unit: IntervalUnit;
}

export const INTERVAL_UNITS: readonly IntervalUnit[] = ['s', 'm', 'h'];

const UNIT_SECONDS: Record<IntervalUnit, number> = { s: 1, m: 60, h: 3600 };

const DEFAULT_INTERVAL: Interval = { count: 5, unit: 'm' };

/** The interval a stored schedule stands for; five minutes when it is not one the backend runs. */
export function parseInterval(cron: string | null | undefined): Interval {
  const match = /^every:(\d+)([smh])$/.exec(cron?.trim() ?? '');
  const count = match ? Number(match[1]) : 0;

  return match && count > 0 ? { count, unit: match[2] as IntervalUnit } : { ...DEFAULT_INTERVAL };
}

export function formatInterval(interval: Interval): string {
  return `every:${Math.max(1, Math.floor(interval.count) || 1)}${interval.unit}`;
}

export function intervalSeconds(interval: Interval): number {
  return Math.max(1, Math.floor(interval.count) || 1) * UNIT_SECONDS[interval.unit];
}

/** A number of seconds in the largest unit that holds it exactly: 300 is 5 minutes, 90 is 90 seconds. */
export function intervalFromSeconds(seconds: number | null | undefined): Interval {
  const total = Math.floor(seconds ?? 0);
  if (!(total > 0)) return { ...DEFAULT_INTERVAL };

  if (total % 3600 === 0) return { count: total / 3600, unit: 'h' };
  if (total % 60 === 0) return { count: total / 60, unit: 'm' };
  return { count: total, unit: 's' };
}
