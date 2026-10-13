import { resolveWindow, type TimeWindow } from '@/stores/useTimeStore';

/** A stretch of time, in microseconds. */
export interface Period {
  from: number;
  to: number;
}

/** A baseline said by where it lies from the period it is compared with. */
export type BaselineRelation = 'previous' | 'day' | 'week';

export const BASELINE_RELATIONS: BaselineRelation[] = ['previous', 'day', 'week'];

/**
 * What a period is compared against: a stretch that follows the period (the one
 * just before it, or the same one a day or a week earlier), or a time range of
 * its own, set the way any time range is.
 */
export type CompareBaseline =
  | { kind: BaselineRelation }
  | { kind: 'window'; window: TimeWindow };

const DAY_MICROS = 24 * 3_600 * 1_000_000;

function span(period: Period): number {
  return Math.max(1, period.to - period.from);
}

/** The stretch a time range stands for at a moment. */
export function windowPeriod(window: TimeWindow, now: Date = new Date()): Period {
  const { from, to } = resolveWindow(window, now);

  return { from: from.getTime() * 1000, to: to.getTime() * 1000 };
}

/** A stretch as a time range with a fixed start and end. */
export function periodWindow(period: Period): TimeWindow {
  return {
    from: new Date(period.from / 1000).toISOString(),
    to: new Date(period.to / 1000).toISOString(),
    mode: 'absolute',
  };
}

/** How far back from a period a related baseline is taken. */
function shiftOf(relation: BaselineRelation, period: Period): number {
  if (relation === 'day') return DAY_MICROS;
  if (relation === 'week') return 7 * DAY_MICROS;

  return span(period);
}

/**
 * Whether a related baseline says anything about a period. One that overlaps it
 * does not: "a day earlier" of a three-day period is mostly the period itself.
 */
export function relationFits(relation: BaselineRelation, period: Period): boolean {
  return shiftOf(relation, period) >= span(period);
}

/**
 * The baseline as it is used for a period: a relation that would overlap the
 * period gives way to the stretch just before it.
 */
export function settledBaseline(baseline: CompareBaseline, comparison: Period): CompareBaseline {
  if (baseline.kind === 'window' || relationFits(baseline.kind, comparison)) return baseline;

  return { kind: 'previous' };
}

/** The period looked at and the one it is set against, at a moment. */
export function comparePeriods(
  comparison: Period,
  baseline: CompareBaseline,
  now: Date = new Date(),
): { comparison: Period; baseline: Period } {
  const settled = settledBaseline(baseline, comparison);
  if (settled.kind === 'window') return { comparison, baseline: windowPeriod(settled.window, now) };

  const shift = shiftOf(settled.kind, comparison);

  return {
    comparison,
    baseline: { from: comparison.from - shift, to: comparison.to - shift },
  };
}

/** Two stretches that share time: what was captured in the shared part counts on both sides. */
export function periodsOverlap(a: Period, b: Period): boolean {
  return a.from < b.to && b.from < a.to;
}
