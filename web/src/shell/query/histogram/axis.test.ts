import { describe, expect, it } from 'vitest';

import {
  type AxisClock,
  binWidthParts,
  countTicks,
  frameDays,
  momentLabel,
  tickLabel,
  tickLabelLength,
  tickStepFor,
  timeTicks,
  utcOffsetMicros,
} from './axis';

const MS = 1_000;
const SECOND = 1_000_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const utc: AxisClock = { tz: 'UTC', format: 'iso_24h', dateFormat: 'yyyy_mm_dd_dash' };
const micros = (iso: string) => Date.parse(iso) * 1_000;

describe('countTicks', () => {
  it('counts in whole numbers from zero to a line no bar passes', () => {
    expect(countTicks(1, 4)).toEqual([0, 1]);
    expect(countTicks(3, 4)).toEqual([0, 1, 2, 3]);
    expect(countTicks(4, 3)).toEqual([0, 2, 4]);
    expect(countTicks(7, 4)).toEqual([0, 2, 4, 6, 8]);
    expect(countTicks(17, 4)).toEqual([0, 5, 10, 15, 20]);
    expect(countTicks(180, 4)).toEqual([0, 50, 100, 150, 200]);
    expect(countTicks(2_300, 3)).toEqual([0, 1_000, 2_000, 3_000]);
  });

  it('never steps by a fraction, however small the count', () => {
    for (const max of [0, 0.4, 1, 2]) {
      expect(countTicks(max, 4).every(Number.isInteger)).toBe(true);
    }
    expect(countTicks(0, 4)).toEqual([0, 1]);
  });

  it('keeps to the number of steps there is room for', () => {
    for (let max = 1; max <= 5_000; max += 7) {
      const ticks = countTicks(max, 3);

      expect(ticks.length - 1).toBeLessThanOrEqual(3);
      expect(ticks.at(-1)).toBeGreaterThanOrEqual(max);
    }
  });
});

describe('the time axis', () => {
  it('labels the edges of bars, at a step a reader counts in', () => {
    const frame = { start: micros('2026-10-11T23:22:46Z'), end: micros('2026-10-11T23:23:25Z') };
    const ticks = timeTicks(frame, SECOND, 5, utc);

    expect(ticks.map((tick) => tick.label)).toEqual(['23:22:50', '23:23:00', '23:23:10', '23:23:20']);
    // Each is a whole number of bars from the start of the frame.
    for (const tick of ticks) expect((tick.micros - frame.start) % SECOND).toBe(0);
  });

  it('steps by a whole number of bars even where the nearest round step is not one', () => {
    const frame = { start: 0, end: 60 * SECOND };

    // Two-second bars: five seconds would fall in the middle of one.
    expect(tickStepFor(frame, 2 * SECOND, 8)).toBe(10 * SECOND);
    expect(tickStepFor(frame, 5 * SECOND, 8)).toBe(10 * SECOND);
    expect(tickStepFor(frame, 5 * SECOND, 20)).toBe(5 * SECOND);
  });

  it('counts in bars where no listed step fits', () => {
    const week = 7 * DAY;
    const step = tickStepFor({ start: 0, end: 80 * week }, week, 5);

    expect(step % week).toBe(0);
    expect(Math.floor((80 * week) / step) + 1).toBeLessThanOrEqual(5);
  });

  it('says the time as fine as the step needs', () => {
    const moment = micros('2026-10-11T08:05:09.250Z');

    expect(tickLabel(moment, 10 * MINUTE, utc).label).toBe('08:05');
    expect(tickLabel(moment, 5 * SECOND, utc).label).toBe('08:05:09');
    expect(tickLabel(moment, 50 * MS, utc).label).toBe('08:05:09.250');
    expect(tickLabel(moment, DAY, utc)).toEqual({ label: '10-11', day: true });
    expect(tickLabel(moment, 5 * SECOND, { ...utc, format: 'local_12h' }).label).toBe('08:05:09 AM');
  });

  it('says the fraction alone between two seconds, and the second where one begins', () => {
    const frame = { start: micros('2026-10-11T09:59:55.700Z'), end: micros('2026-10-11T09:59:57.200Z') };

    expect(timeTicks(frame, 100 * MS, 4, utc).map((tick) => tick.label)).toEqual([
      '09:59:56',
      '.500',
      '09:59:57',
    ]);
    expect(
      timeTicks({ start: frame.start + 400 * MS, end: frame.end }, 100 * MS, 4, utc).map((tick) => tick.label),
    ).toEqual(['09:59:56.500', '09:59:57']);
  });

  it('names the day where one begins', () => {
    const frame = { start: micros('2026-10-11T22:00:00Z'), end: micros('2026-10-12T02:00:00Z') };
    const ticks = timeTicks(frame, HOUR, 5, utc);

    expect(ticks.map((tick) => tick.label)).toEqual(['22:00', '23:00', '10-12', '01:00', '02:00']);
    expect(ticks.map((tick) => tick.day)).toEqual([false, false, true, false, false]);
  });

  it('is of the reader\'s clock: the same instants read differently in another zone', () => {
    const frame = { start: micros('2026-10-11T15:00:00Z'), end: micros('2026-10-11T18:00:00Z') };
    const shanghai = { ...utc, tz: 'Asia/Shanghai' };

    expect(timeTicks(frame, HOUR, 4, shanghai).map((tick) => tick.label)).toEqual([
      '23:00',
      '10-12',
      '01:00',
      '02:00',
    ]);
    expect(utcOffsetMicros(frame.start, 'Asia/Shanghai')).toBe(8 * HOUR);
    expect(utcOffsetMicros(frame.start, 'Asia/Kolkata')).toBe(5 * HOUR + 30 * MINUTE);

    // Half-hour zones: an hour mark is on the hour there, at half past in UTC.
    const kolkata = timeTicks(frame, HOUR, 4, { ...utc, tz: 'Asia/Kolkata' });
    expect(kolkata.map((tick) => tick.label)).toEqual(['21:00', '22:00', '23:00']);
  });

  it('writes a day as the reader writes dates', () => {
    const moment = micros('2026-03-09T00:00:00Z');

    expect(tickLabel(moment, HOUR, { ...utc, dateFormat: 'dd_mm_yyyy_slash' }).label).toBe('09/03');
    expect(tickLabel(moment, HOUR, { ...utc, dateFormat: 'mm_dd_yyyy_slash' }).label).toBe('03/09');
    expect(tickLabel(moment, HOUR, { ...utc, dateFormat: 'yyyy_mm_dd_slash' }).label).toBe('03/09');
  });

  it('knows how wide its labels are', () => {
    expect(tickLabelLength(MINUTE, utc)).toBe('08:05'.length);
    expect(tickLabelLength(SECOND, utc)).toBe('08:05:09'.length);
    expect(tickLabelLength(SECOND, { ...utc, format: 'local_12h' })).toBe('08:05:09 AM'.length);
    // Most marks under a second are shorter than the time in full.
    expect(tickLabelLength(10 * MS, utc)).toBe('08:05:09'.length);
    expect(tickLabelLength(DAY, utc)).toBe('10-11'.length);
  });
});

describe('what stands beside the axis', () => {
  it('names the day the frame lies on, once', () => {
    expect(
      frameDays({ start: micros('2026-10-11T23:22:46Z'), end: micros('2026-10-11T23:23:25Z') }, utc),
    ).toEqual(['2026-10-11']);
  });

  it('does not count the instant the last bar ends on as another day', () => {
    expect(
      frameDays({ start: micros('2026-10-11T23:00:00Z'), end: micros('2026-10-12T00:00:00Z') }, utc),
    ).toEqual(['2026-10-11']);
  });

  it('names both days when the frame runs into the next', () => {
    expect(
      frameDays({ start: micros('2026-10-11T23:00:00Z'), end: micros('2026-10-12T00:30:00Z') }, utc),
    ).toEqual(['2026-10-11', '2026-10-12']);
  });

  it('says the length of a bar in the largest unit it is a whole number of', () => {
    expect(binWidthParts(500 * MS)).toEqual({ value: 500, unit: 'ms' });
    expect(binWidthParts(SECOND)).toEqual({ value: 1, unit: 's' });
    expect(binWidthParts(30 * SECOND)).toEqual({ value: 30, unit: 's' });
    expect(binWidthParts(MINUTE)).toEqual({ value: 1, unit: 'min' });
    expect(binWidthParts(3 * HOUR)).toEqual({ value: 3, unit: 'h' });
    expect(binWidthParts(14 * DAY)).toEqual({ value: 14, unit: 'd' });
  });

  it('says an edge of a bar as fine as the bars are', () => {
    const moment = micros('2026-10-11T23:23:01Z');

    expect(momentLabel(moment, SECOND, utc, false)).toBe('23:23:01');
    expect(momentLabel(moment, SECOND, utc, true)).toBe('2026-10-11 23:23:01');
    expect(momentLabel(moment, 5 * MINUTE, utc, false)).toBe('23:23');
    expect(momentLabel(moment, 100 * MS, utc, false)).toBe('23:23:01.000');
    expect(momentLabel(moment, DAY, utc, false)).toBe('2026-10-11');
  });
});
