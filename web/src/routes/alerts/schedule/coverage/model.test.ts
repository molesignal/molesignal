import { describe, expect, it } from 'vitest';

import '@/lib/time';
import type { CoverageSegment } from '@/types/alerting';

import { coverageDays, coverageGaps, gapHours } from './model';

const HOUR = 3_600_000_000;
const NOON = Date.UTC(2026, 4, 23, 12) * 1000;
const MIDNIGHT = Date.UTC(2026, 4, 23) * 1000;

const segment = (source: CoverageSegment['source'], from: number, to: number, user_id?: string): CoverageSegment => ({
  source,
  startAt: from,
  endAt: to,
  ...(user_id ? { userId: user_id } : {}),
});

describe('coverageDays', () => {
  it('lays out days of 24 hours from the start of today, in the schedule’s timezone', () => {
    const days = coverageDays([], NOON, 2, 'UTC');

    expect(days).toHaveLength(2);
    expect(days[0]?.start).toBe(MIDNIGHT);
    expect(days[1]?.start).toBe(MIDNIGHT + 24 * HOUR);
    expect(days[0]?.cells).toHaveLength(24);
  });

  it('starts the day where that timezone starts it', () => {
    const [day] = coverageDays([], NOON, 1, 'Asia/Shanghai');
    expect(day?.start).toBe(Date.UTC(2026, 4, 22, 16) * 1000);
  });

  it('keeps every day at midnight across a change of clocks', () => {
    const local = (micros: number) =>
      new Date(micros / 1000).toLocaleString('sv-SE', { timeZone: 'America/New_York' });
    const fallBack = coverageDays([], Date.UTC(2026, 9, 30, 12) * 1000, 5, 'America/New_York');
    const springForward = coverageDays([], Date.UTC(2026, 2, 6, 12) * 1000, 5, 'America/New_York');

    expect(fallBack.map((day) => local(day.start))).toEqual([
      '2026-10-30 00:00:00',
      '2026-10-31 00:00:00',
      '2026-11-01 00:00:00',
      '2026-11-02 00:00:00',
      '2026-11-03 00:00:00',
    ]);
    expect(springForward.map((day) => local(day.start)).slice(2)).toEqual([
      '2026-03-08 00:00:00',
      '2026-03-09 00:00:00',
      '2026-03-10 00:00:00',
    ]);
  });

  it('says who is on, and calls an hour with any hole in it a gap', () => {
    const [day] = coverageDays(
      [
        segment('rotation', MIDNIGHT, MIDNIGHT + 2 * HOUR + HOUR / 2, 'kenji'),
        segment('gap', MIDNIGHT + 2 * HOUR + HOUR / 2, MIDNIGHT + 4 * HOUR),
      ],
      NOON,
      1,
      'UTC',
    );

    expect(day?.cells[0]).toMatchObject({ source: 'rotation', userId: 'kenji' });
    expect(day?.cells[2]?.source).toBe('gap');
    expect(day?.cells[3]?.source).toBe('gap');
    expect(day?.cells[5]?.source).toBeNull();
  });
});

describe('coverageGaps', () => {
  it('takes only what is ahead, within the horizon', () => {
    const gaps = coverageGaps(
      [
        segment('gap', NOON - 5 * HOUR, NOON - 4 * HOUR),
        segment('gap', NOON - HOUR, NOON + HOUR),
        segment('gap', NOON + 10 * HOUR, NOON + 50 * HOUR),
      ],
      NOON,
      NOON + 20 * HOUR,
    );

    expect(gaps).toEqual([
      { from: NOON, to: NOON + HOUR },
      { from: NOON + 10 * HOUR, to: NOON + 20 * HOUR },
    ]);
    expect(gapHours(gaps)).toBe(11);
  });

  it('takes stretches that touch as one', () => {
    expect(
      coverageGaps([segment('gap', NOON, NOON + HOUR), segment('gap', NOON + HOUR, NOON + 2 * HOUR)], NOON, NOON + 3 * HOUR),
    ).toEqual([{ from: NOON, to: NOON + 2 * HOUR }]);
  });

  it('finds nothing where someone is always on', () => {
    expect(coverageGaps([segment('rotation', NOON, NOON + HOUR, 'kenji')], NOON, NOON + HOUR)).toEqual([]);
  });
});
