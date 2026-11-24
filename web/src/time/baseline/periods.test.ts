import { describe, expect, it } from 'vitest';

import {
  comparePeriods,
  type Period,
  periodsOverlap,
  periodWindow,
  relationFits,
  settledBaseline,
  windowPeriod,
} from './periods';

const HOUR = 3_600 * 1_000_000;
const DAY = 24 * HOUR;

/** An hour that ends at ten o'clock of some day. */
const hour: Period = { from: 9 * HOUR + 10 * DAY, to: 10 * HOUR + 10 * DAY };
const threeDays: Period = { from: 7 * DAY, to: 10 * DAY };

describe('comparePeriods', () => {
  it('takes the stretch just before the period as its baseline', () => {
    expect(comparePeriods(hour, { kind: 'previous' })).toEqual({
      comparison: hour,
      baseline: { from: hour.from - HOUR, to: hour.from },
    });
  });

  it('takes the same stretch a day or a week earlier', () => {
    expect(comparePeriods(hour, { kind: 'day' }).baseline).toEqual({
      from: hour.from - DAY,
      to: hour.to - DAY,
    });
    expect(comparePeriods(hour, { kind: 'week' }).baseline).toEqual({
      from: hour.from - 7 * DAY,
      to: hour.to - 7 * DAY,
    });
  });

  it('takes a time range of its own as the baseline, wherever it lies', () => {
    const own: Period = { from: 2 * DAY, to: 2 * DAY + 20 * 60 * 1_000_000 };

    expect(comparePeriods(hour, { kind: 'window', window: periodWindow(own) }).baseline).toEqual(own);
    // Later than the period it is compared with is a baseline too.
    const later: Period = { from: hour.to + HOUR, to: hour.to + 2 * HOUR };
    expect(comparePeriods(hour, { kind: 'window', window: periodWindow(later) }).baseline).toEqual(later);
  });

  it('reads a quick range of the baseline against the moment it is asked at', () => {
    const now = new Date('2026-10-12T16:00:00Z');
    const lastSixHours = { from: 'now-6h', to: 'now', mode: 'relative' } as const;

    expect(comparePeriods(hour, { kind: 'window', window: lastSixHours }, now).baseline).toEqual({
      from: (now.getTime() - 6 * 3_600_000) * 1000,
      to: now.getTime() * 1000,
    });
    expect(windowPeriod(lastSixHours, now)).toEqual({
      from: (now.getTime() - 6 * 3_600_000) * 1000,
      to: now.getTime() * 1000,
    });
  });

  it('writes a stretch as a range with a fixed start and end, and reads it back', () => {
    const own: Period = { from: Date.parse('2026-10-01T08:00:00Z') * 1000, to: Date.parse('2026-10-01T09:30:00Z') * 1000 };

    expect(periodWindow(own)).toEqual({
      from: '2026-10-01T08:00:00.000Z',
      to: '2026-10-01T09:30:00.000Z',
      mode: 'absolute',
    });
    expect(windowPeriod(periodWindow(own))).toEqual(own);
  });

  it('does not compare a period with a day earlier that is mostly the period itself', () => {
    expect(relationFits('day', hour)).toBe(true);
    expect(relationFits('day', threeDays)).toBe(false);
    expect(relationFits('week', threeDays)).toBe(true);
    expect(relationFits('previous', threeDays)).toBe(true);

    expect(settledBaseline({ kind: 'day' }, threeDays)).toEqual({ kind: 'previous' });
    expect(comparePeriods(threeDays, { kind: 'day' }).baseline).toEqual({ from: 4 * DAY, to: 7 * DAY });
  });

  it('keeps a relation that just fits: a day compared with the day before', () => {
    const day: Period = { from: 9 * DAY, to: 10 * DAY };

    expect(settledBaseline({ kind: 'day' }, day)).toEqual({ kind: 'day' });
    expect(comparePeriods(day, { kind: 'day' }).baseline).toEqual({ from: 8 * DAY, to: 9 * DAY });
  });
});

describe('periodsOverlap', () => {
  it('is said of stretches that share time, not of ones that only touch', () => {
    expect(periodsOverlap(hour, { from: hour.from - HOUR, to: hour.from })).toBe(false);
    expect(periodsOverlap(hour, { from: hour.from - HOUR, to: hour.from + 1 })).toBe(true);
    expect(periodsOverlap(hour, { from: hour.from + 1, to: hour.to - 1 })).toBe(true);
    expect(periodsOverlap(hour, { from: hour.to, to: hour.to + HOUR })).toBe(false);
  });
});
