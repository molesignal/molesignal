import dayjs from 'dayjs';

import type { CoverageSegment, CoverageSource } from '@/types/alerting';

const HOUR_MICROS = 3_600_000_000;

export interface CoverageCell {
  start: number;
  end: number;
  source: CoverageSource | null;
  userId?: string | undefined;
}

export interface CoverageDay {
  /** Micros at the day's first hour, in the schedule's timezone. */
  start: number;
  cells: CoverageCell[];
}

export interface CoverageGap {
  from: number;
  to: number;
}

const RANK: Record<CoverageSource, number> = { rotation: 0, override: 1, gap: 2 };

/**
 * Who stands behind each hour. An hour with a hole in it counts as a gap, however
 * short: that is the one thing the grid is there to show.
 */
function cellAt(segments: readonly CoverageSegment[], start: number, end: number): CoverageCell {
  const overlapping = segments.filter((segment) => segment.startAt < end && segment.endAt > start);
  const worst = overlapping.reduce<CoverageSegment | null>(
    (held, segment) => (!held || RANK[segment.source] > RANK[held.source] ? segment : held),
    null,
  );

  return { start, end, source: worst?.source ?? null, userId: worst?.userId };
}

/**
 * Hour by hour, in days of the schedule's timezone, starting with today. Each day
 * is found by its date: adding days to a moment keeps its offset, so every day
 * after a change of clocks would start an hour off. The day of the change is
 * still drawn as 24 hours from its midnight.
 */
export function coverageDays(
  segments: readonly CoverageSegment[],
  nowMicros: number,
  dayCount: number,
  timeZone: string,
): CoverageDay[] {
  const today = dayjs.utc(dayjs(nowMicros / 1000).tz(timeZone).format('YYYY-MM-DD'));

  return Array.from({ length: dayCount }, (_, dayIndex) => {
    const start = dayjs.tz(today.add(dayIndex, 'day').format('YYYY-MM-DD'), timeZone).valueOf() * 1000;

    return {
      start,
      cells: Array.from({ length: 24 }, (_, hour) => {
        const from = start + hour * HOUR_MICROS;
        return cellAt(segments, from, from + HOUR_MICROS);
      }),
    };
  });
}

/** The stretches nobody is on call from now until `untilMicros`, those that touch taken as one. */
export function coverageGaps(
  segments: readonly CoverageSegment[],
  nowMicros: number,
  untilMicros: number,
): CoverageGap[] {
  const gaps: CoverageGap[] = [];

  for (const segment of [...segments].sort((a, b) => a.startAt - b.startAt)) {
    if (segment.source !== 'gap') continue;

    const from = Math.max(segment.startAt, nowMicros);
    const to = Math.min(segment.endAt, untilMicros);
    if (to <= from) continue;

    const last = gaps.at(-1);
    if (last && last.to >= from) last.to = Math.max(last.to, to);
    else gaps.push({ from, to });
  }

  return gaps;
}

export function gapHours(gaps: readonly CoverageGap[]): number {
  const micros = gaps.reduce((sum, gap) => sum + (gap.to - gap.from), 0);
  return Math.round((micros / HOUR_MICROS) * 10) / 10;
}
