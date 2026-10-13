import { describe, expect, it } from 'vitest';

import {
  alignDown,
  alignUp,
  barIndexAt,
  binHistogram,
  binWidthFor,
  fitHistogramRange,
  histogramFromCounts,
  planHistogram,
  TIME_STEPS,
} from './model';

const MS = 1_000;
const SECOND = 1_000_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const at = (...micros: Array<number | null>) => micros.map((value) => ({ micros: value, error: false }));

describe('binWidthFor', () => {
  it('takes the shortest length of time a reader counts in that keeps the bars few enough', () => {
    // Twenty-six seconds in at most a hundred bars: half a second each, not 0.26 s.
    expect(binWidthFor(26 * SECOND, 100)).toBe(500 * MS);
    expect(binWidthFor(HOUR, 80)).toBe(MINUTE);
    expect(binWidthFor(DAY, 60)).toBe(30 * MINUTE);
    expect(binWidthFor(SECOND, 120)).toBe(10 * MS);
    expect(binWidthFor(40 * DAY, 50)).toBe(DAY);
  });

  it('counts in whole years past the end of the list', () => {
    const year = TIME_STEPS.at(-1)!;

    expect(binWidthFor(30 * year, 10)).toBe(3 * year);
  });

  it('lists lengths that only grow', () => {
    expect([...TIME_STEPS].sort((left, right) => left - right)).toEqual([...TIME_STEPS]);
  });
});

describe('binHistogram', () => {
  it('counts events into bars of one length that begin on the clock, errors apart from the rest', () => {
    const histogram = binHistogram(
      [
        { micros: 10 * SECOND + 300 * MS, error: false },
        { micros: 10 * SECOND + 900 * MS, error: false },
        { micros: 12 * SECOND + 100 * MS, error: true },
        { micros: 14 * SECOND + 999 * MS, error: false },
      ],
      { start: 10 * SECOND + 250 * MS, end: 14 * SECOND + 999 * MS + 1 },
      { maxBins: 5 },
    );

    // Not 4.75 s cut in five: a second a bar, from :10 to :15.
    expect(histogram?.binMicros).toBe(SECOND);
    expect(histogram?.frame).toEqual({ start: 10 * SECOND, end: 15 * SECOND });
    expect(histogram?.buckets.map((bucket) => [bucket.ok, bucket.err])).toEqual([
      [2, 0],
      [0, 0],
      [0, 1],
      [0, 0],
      [1, 0],
    ]);
    expect(histogram?.buckets[2]).toMatchObject({ startMicros: 12 * SECOND, endMicros: 13 * SECOND });
  });

  it('begins a bar on the reader\'s clock, not on UTC', () => {
    // Half past five ahead of UTC: an hour on that clock begins at half past in UTC.
    const offset = 5 * HOUR + 30 * MINUTE;
    const histogram = binHistogram(at(2 * HOUR, 9 * HOUR), { start: 2 * HOUR, end: 9 * HOUR }, {
      maxBins: 8,
      utcOffsetMicros: offset,
    });

    expect(histogram?.binMicros).toBe(HOUR);
    expect((histogram!.frame.start + offset) % HOUR).toBe(0);
    expect(histogram?.frame.start).toBe(HOUR + 30 * MINUTE);
  });

  it('counts an event outside the range in the bar at that end of it', () => {
    const histogram = binHistogram(
      [
        { micros: -40 * SECOND, error: false },
        { micros: 400 * SECOND, error: true },
      ],
      { start: 0, end: 100 * SECOND },
      { maxBins: 10 },
    );

    expect(histogram?.buckets[0]?.ok).toBe(1);
    expect(histogram?.buckets.at(-1)?.err).toBe(1);
  });

  it('spans the events themselves when there is no range', () => {
    const histogram = binHistogram(at(SECOND, 3 * SECOND), null, { maxBins: 4 });

    expect(histogram?.binMicros).toBe(SECOND);
    expect(histogram?.frame).toEqual({ start: SECOND, end: 4 * SECOND });
    expect(histogram?.buckets.map((bucket) => bucket.ok)).toEqual([1, 0, 1]);
  });

  it('leaves out an event that carries no time, and draws nothing when none does', () => {
    const histogram = binHistogram(at(null, 2 * SECOND, null), { start: 0, end: 4 * SECOND }, { maxBins: 4 });

    expect(histogram?.buckets.reduce((total, bucket) => total + bucket.ok, 0)).toBe(1);
    expect(binHistogram(at(null, null), { start: 0, end: 4 * SECOND }, { maxBins: 4 })).toBeNull();
    expect(binHistogram([], null, { maxBins: 4 })).toBeNull();
  });

  it('always has a bar to draw', () => {
    // One instant, and a range that ends on the edge of a bar.
    const histogram = binHistogram(at(5 * SECOND), { start: 5 * SECOND, end: 5 * SECOND + 1 }, { maxBins: 10 });

    expect(histogram?.buckets).toHaveLength(1);
    expect(histogram?.buckets[0]?.ok).toBe(1);
  });
});

describe('planHistogram', () => {
  it('settles the bars of a range before anything is counted into them', () => {
    // Not 4.75 s cut in five: a second a bar, from :10 to :15.
    expect(planHistogram({ start: 10 * SECOND + 250 * MS, end: 14 * SECOND + 999 * MS + 1 }, { maxBins: 5 })).toEqual({
      binMicros: SECOND,
      frame: { start: 10 * SECOND, end: 15 * SECOND },
    });
    // A day drawn in at most a hundred bars is drawn in quarters of an hour.
    expect(planHistogram({ start: 0, end: DAY }, { maxBins: 100 })).toEqual({
      binMicros: 15 * MINUTE,
      frame: { start: 0, end: DAY },
    });
  });

  it('begins the first bar on the reader\'s clock, at or before the start of the range', () => {
    const offset = 5 * HOUR + 30 * MINUTE;
    const plan = planHistogram({ start: 2 * HOUR, end: 9 * HOUR }, { maxBins: 8, utcOffsetMicros: offset });

    expect(plan.binMicros).toBe(HOUR);
    expect(plan.frame).toEqual({ start: HOUR + 30 * MINUTE, end: 9 * HOUR + 30 * MINUTE });
  });

  it('plans the bars that the loaded results are counted into', () => {
    const range = { start: 0, end: 100 * SECOND };
    const histogram = binHistogram(at(5 * SECOND), range, { maxBins: 10 });

    expect({ binMicros: histogram?.binMicros, frame: histogram?.frame }).toEqual(planHistogram(range, { maxBins: 10 }));
  });
});

describe('histogramFromCounts', () => {
  const plan = { binMicros: SECOND, frame: { start: 10 * SECOND, end: 15 * SECOND } };

  it('fills the bars of the plan with the counts made for them, errors apart from the rest', () => {
    const histogram = histogramFromCounts(plan, [
      { startMicros: 12 * SECOND, total: 40, errors: 3 },
      { startMicros: 10 * SECOND, total: 7, errors: 0 },
    ]);

    expect(histogram.binMicros).toBe(SECOND);
    expect(histogram.frame).toEqual(plan.frame);
    expect(histogram.buckets.map((bucket) => [bucket.ok, bucket.err])).toEqual([
      [7, 0],
      [0, 0],
      [37, 3],
      [0, 0],
      [0, 0],
    ]);
    expect(histogram.buckets[2]).toMatchObject({ startMicros: 12 * SECOND, endMicros: 13 * SECOND });
  });

  it('is an empty frame without counts', () => {
    const histogram = histogramFromCounts(plan, []);

    expect(histogram.buckets).toHaveLength(5);
    expect(histogram.buckets.every((bucket) => bucket.ok + bucket.err === 0)).toBe(true);
  });

  it('leaves out a count that was made for other bars', () => {
    const histogram = histogramFromCounts(plan, [
      // Half a second into a bar, before the first bar, and past the last.
      { startMicros: 11 * SECOND + 500 * MS, total: 9, errors: 0 },
      { startMicros: 9 * SECOND, total: 9, errors: 0 },
      { startMicros: 15 * SECOND, total: 9, errors: 0 },
    ]);

    expect(histogram.buckets.reduce((total, bucket) => total + bucket.ok + bucket.err, 0)).toBe(0);
  });

  it('counts no more errors in a bar than the bar holds', () => {
    const histogram = histogramFromCounts(plan, [{ startMicros: 10 * SECOND, total: 4, errors: 9 }]);

    expect(histogram.buckets[0]).toMatchObject({ ok: 0, err: 4 });
  });
});

describe('aligning to the clock', () => {
  it('finds the multiples of a step on either side of a moment', () => {
    expect(alignDown(12_300, 5_000)).toBe(10_000);
    expect(alignUp(12_300, 5_000)).toBe(15_000);
    expect(alignDown(10_000, 5_000)).toBe(10_000);
    expect(alignUp(10_000, 5_000)).toBe(10_000);
  });
});

describe('barIndexAt', () => {
  it('says which bar a point along the plot is on', () => {
    expect(barIndexAt(0, 100, 10)).toBe(0);
    expect(barIndexAt(9.9, 100, 10)).toBe(0);
    expect(barIndexAt(10, 100, 10)).toBe(1);
    expect(barIndexAt(99.9, 100, 10)).toBe(9);
  });

  it('keeps a point past either end on the bar at that end', () => {
    expect(barIndexAt(-5, 100, 10)).toBe(0);
    expect(barIndexAt(100, 100, 10)).toBe(9);
    expect(barIndexAt(50, 0, 10)).toBe(0);
  });
});

describe('fitHistogramRange', () => {
  it('frames a page of results by the results, not by the whole range they were asked from', () => {
    // The newest traces of a day: sixteen seconds at its very end.
    const points = at(DAY - 16 * SECOND, DAY - 9 * SECOND, DAY - SECOND);
    const frame = fitHistogramRange(points, { start: 0, end: DAY });

    // The results' own span and a margin of a twenty-fifth of it at either end.
    expect(frame).toEqual({ start: DAY - 16_600_000, end: DAY - 400_000 });

    // Drawn across that frame the results are a shape; across the day they were one bar.
    const fitted = binHistogram(points, frame, { maxBins: 20 });
    const across = binHistogram(points, { start: 0, end: DAY }, { maxBins: 20 });
    expect(fitted?.buckets.filter((bucket) => bucket.ok > 0)).toHaveLength(3);
    expect(across?.buckets.filter((bucket) => bucket.ok > 0)).toHaveLength(1);
  });

  it('stays inside the range that was asked for', () => {
    expect(fitHistogramRange(at(100, 1_000_000_000), { start: 0, end: 1_000_000_000 })).toEqual({
      start: 0,
      end: 1_000_000_000,
    });
  });

  it('gives one result, or several at one instant, a second to stand in', () => {
    expect(fitHistogramRange(at(5 * SECOND, 5 * SECOND), { start: 0, end: DAY })).toEqual({
      start: 4_460_000,
      end: 5_540_000,
    });
  });

  it('has only the results to go by when no range was asked for', () => {
    expect(fitHistogramRange(at(10 * SECOND, 20 * SECOND), null)).toEqual({
      start: 9_600_000,
      end: 20_400_000,
    });
  });

  it('leaves the range alone when the results carry no time, or all lie outside it', () => {
    const range = { start: 0, end: DAY };

    expect(fitHistogramRange(at(null, null), range)).toBe(range);
    expect(fitHistogramRange([], range)).toBe(range);
    expect(fitHistogramRange(at(DAY * 3), range)).toBe(range);
    expect(fitHistogramRange([], null)).toBeNull();
  });
});
