import { describe, expect, it } from 'vitest';

import {
  analyzeLoadedValues,
  formatShare,
  formatWrittenNumber,
  hasCommonValues,
  planValueBars,
  planValueTrend,
  quantile,
} from './model';

const SECOND = 1_000_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;

describe('planValueBars', () => {
  it('counts into bars of a round width that begin on a multiple of it', () => {
    expect(planValueBars(3, 1_187, true)).toEqual({ origin: 0, width: 50, bars: 24 });
    expect(planValueBars(-7.5, 7.5, false)).toEqual({ origin: -8, width: 1, bars: 16 });
  });

  it('never counts whole numbers into bars narrower than one', () => {
    // 200, 201, 204: bars a fifth wide would leave four in five empty.
    expect(planValueBars(200, 204, true)).toEqual({ origin: 200, width: 1, bars: 5 });
  });

  it('counts fractions into bars as fine as they are', () => {
    const plan = planValueBars(0.012, 0.98, false)!;

    expect(plan.width).toBeCloseTo(0.05, 12);
    expect(plan.origin).toBe(0);
    expect(plan.bars).toBe(20);
  });

  it('has no bars for one value or for what is not a number', () => {
    expect(planValueBars(5, 5, true)).toBeNull();
    expect(planValueBars(Number.NaN, 1, false)).toBeNull();
    expect(planValueBars(0, Infinity, false)).toBeNull();
  });
});

describe('planValueTrend', () => {
  it('counts an hour in five minutes past the hour', () => {
    // Seven past to seven past: the first stretch begins on the five before.
    expect(planValueTrend(7 * MINUTE, HOUR + 7 * MINUTE)).toEqual({
      originMicros: 5 * MINUTE,
      bucketMicros: 5 * MINUTE,
      stretches: 13,
    });
  });

  it('counts a day in half hours', () => {
    expect(planValueTrend(0, 24 * HOUR)).toMatchObject({ bucketMicros: 30 * MINUTE, stretches: 48 });
  });

  it('has no stretches for no range, or for one too long to count in weeks', () => {
    expect(planValueTrend(5, 5)).toBeNull();
    expect(planValueTrend(9, 5)).toBeNull();
    expect(planValueTrend(0, 400 * 7 * 24 * HOUR)).toBeNull();
  });
});

describe('quantile', () => {
  it('lies between the two values it falls between', () => {
    expect(quantile([1, 2, 3, 4], 0.5)).toBe(2.5);
    expect(quantile([1, 2, 3, 4, 5], 0.25)).toBe(2);
    expect(quantile([7], 0.75)).toBe(7);
    expect(quantile([], 0.5)).toBeNull();
  });
});

describe('analyzeLoadedValues', () => {
  it('counts a number, ranks its values and measures its spread', () => {
    const analysis = analyzeLoadedValues([200, 200, 200, 404, 500, null, 503, ''], 'number');

    expect(analysis).toMatchObject({ total: 8, present: 6, distinct: 4, distinctExact: true });
    expect(analysis.top).toEqual([
      { value: '200', count: 3 },
      { value: '404', count: 1 },
      { value: '500', count: 1 },
      { value: '503', count: 1 },
    ]);
    expect(analysis.range).toEqual({ min: '200', max: '503' });
    expect(analysis.spread!.avg).toBeCloseTo(334.5, 9);
    expect(analysis.spread!.median).toBe(302);
    expect(analysis.spread!.stddevSamp!).toBeGreaterThan(analysis.spread!.stddevPop!);
    // Half the values are 200 and the rest lie far above: the tail is to the right.
    expect(analysis.spread!.skewness!).toBeGreaterThan(0);
    expect(analysis.bars).toMatchObject({ origin: 200, width: 20 });
    expect(analysis.bars!.counts).toHaveLength(16);
    expect([analysis.bars!.counts[0], analysis.bars!.counts[10], analysis.bars!.counts[15]]).toEqual([3, 1, 2]);
  });

  it('reads numbers that arrive as text, and leaves out what is not one', () => {
    const analysis = analyzeLoadedValues(['1', '2', '2', 'n/a'], 'number');

    expect(analysis.present).toBe(4);
    expect(analysis.range).toEqual({ min: '1', max: '2' });
    expect(analysis.spread!.avg).toBeCloseTo(5 / 3, 9);
  });

  it('gives the shape of values about their mean', () => {
    // 1, 2, 2, 3: even on both sides, and flatter than a normal distribution.
    const { spread } = analyzeLoadedValues([1, 2, 2, 3], 'number');

    expect(spread!.skewness).toBeCloseTo(0, 12);
    expect(spread!.kurtosis).toBeCloseTo(-1, 12);
    expect(spread!.stddevPop).toBeCloseTo(Math.SQRT1_2, 12);
  });

  it('has no spread to speak of for one value', () => {
    const { spread, bars } = analyzeLoadedValues([7], 'number');

    expect(spread).toMatchObject({ avg: 7, median: 7, stddevSamp: null, skewness: null, kurtosis: null });
    expect(bars).toBeNull();
  });

  it('counts text without measuring it, and an empty string is no value', () => {
    const analysis = analyzeLoadedValues(['api', 'api', 'web', '', undefined], 'text');

    expect(analysis).toMatchObject({ total: 5, present: 3, distinct: 2, range: null, spread: null, bars: null });
    expect(analysis.top).toEqual([
      { value: 'api', count: 2 },
      { value: 'web', count: 1 },
    ]);
    expect(hasCommonValues(analysis)).toBe(true);
  });

  it('gives a moment an earliest and a latest, and no average', () => {
    const analysis = analyzeLoadedValues([3_000, 1_000, '1970-01-01T00:00:02Z'], 'time');

    expect(analysis.range).toEqual({ min: '1000', max: '2000000' });
    expect(analysis.spread).toBeNull();
  });

  it('follows the commonest values over time, each in the stretch it fell in', () => {
    const values = ['api', 'api', 'web', 'api', 'db', null];
    const micros = [1 * SECOND, 2 * SECOND, 2 * SECOND, 9 * SECOND, 4 * SECOND, 5 * SECOND];
    const { trend } = analyzeLoadedValues(values, 'text', { micros, range: { start: 0, end: 10 * SECOND } });

    expect(trend).toMatchObject({ originMicros: 0, bucketMicros: SECOND });
    expect(trend!.series.map((item) => item.value)).toEqual(['api', 'web', 'db']);
    expect(trend!.series[0]!.counts).toEqual([0, 1, 1, 0, 0, 0, 0, 0, 0, 1]);
    expect(trend!.series[2]!.counts).toEqual([0, 0, 0, 0, 1, 0, 0, 0, 0, 0]);
  });

  it('follows no more than five values, and none where each is met once', () => {
    const many = Array.from({ length: 14 }, (_, index) => `v${index % 7}`);
    const when = { micros: many.map((_, index) => index * SECOND), range: { start: 0, end: 20 * SECOND } };

    expect(analyzeLoadedValues(many, 'text', when).trend!.series).toHaveLength(5);
    expect(analyzeLoadedValues(['a', 'b', 'c'], 'text', { ...when, micros: [1, 2, 3] }).trend).toBeNull();
    // A moment is not followed over time: its values are the time.
    expect(analyzeLoadedValues([1, 1, 2], 'time', { ...when, micros: [1, 2, 3] }).trend).toBeNull();
    // Without the moments of the rows there is nothing to follow them over.
    expect(analyzeLoadedValues(many, 'text').trend).toBeNull();
  });

  it('says so when no value is commoner than the rest', () => {
    const analysis = analyzeLoadedValues(['a', 'b', 'c'], 'text');

    expect(hasCommonValues(analysis)).toBe(false);
    expect(hasCommonValues({ ...analysis, top: [{ value: 'a', count: 9 }], topSkipped: true })).toBe(false);
  });
});

describe('formatShare', () => {
  it('is a part of a whole as a percentage, and nothing of no whole', () => {
    expect(formatShare(1, 4)).toBe((0.25).toLocaleString(undefined, { style: 'percent', maximumFractionDigits: 1 }));
    expect(formatShare(3, 0)).toBe((0).toLocaleString(undefined, { style: 'percent', maximumFractionDigits: 1 }));
  });
});

describe('formatWrittenNumber', () => {
  it('groups the digits of a whole number without losing any', () => {
    expect(formatWrittenNumber('1700000000123')).toBe((1_700_000_000_123).toLocaleString());
  });

  it('leaves what a double cannot hold exactly as it was written', () => {
    expect(formatWrittenNumber('1700000000123456789')).toBe('1700000000123456789');
    expect(formatWrittenNumber('GET')).toBe('GET');
    expect(formatWrittenNumber('')).toBe('');
  });
});
