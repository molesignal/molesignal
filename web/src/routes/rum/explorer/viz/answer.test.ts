import { describe, expect, it } from 'vitest';

import type { RumAggregate } from '@/api/rum';

import {
  chartSeries,
  countryValues,
  formatMeasure,
  groupLabel,
  groupsLabel,
  rankedRows,
  seriesGroups,
  vitalRescale,
  vitalThresholds,
} from './answer';
import { bucketText, spanText } from './format';
import { COUNT, type RumVizBars } from './spec';

const HOUR = 3_600_000_000;
const MINUTE = 60_000_000;
const BARS: RumVizBars = { binMicros: HOUR, frame: { start: 0, end: 4 * HOUR } };

const answer = (rows: RumAggregate['rows'], measures: RumAggregate['measures'], groupBy: string[] = ['browser.name']): RumAggregate => ({
  groupBy: groupBy,
  measures,
  bucketTime: HOUR,
  originTime: 0,
  rows,
});

const COUNTS: RumAggregate['measures'] = [{ name: 'count', fn: 'count', unit: 'count' }];
const LCP: RumAggregate['measures'] = [{ name: 'p75(view.performance.lcp.timestamp)', fn: 'p75', field: 'view.performance.lcp.timestamp', unit: 'ns' }];

const inputs = {
  bars: BARS,
  groups: ['browser.name'],
  language: 'en',
  none: '(none)',
  measureLabel: () => 'count of views',
};

describe('the lines of a chart over time', () => {
  const counted = answer(
    [
      { group: ['Safari'], startTime: HOUR, values: [1] },
      { group: ['Chrome'], startTime: 0, values: [5] },
      { group: ['Chrome'], startTime: HOUR, values: [3] },
      { group: ['Chrome'], startTime: 3 * HOUR, values: [2] },
    ],
    COUNTS,
  );

  it('are one to a group, the one with most first, over every bar of the frame', () => {
    const series = chartSeries(counted, { ...inputs, measures: [COUNT] });

    expect(series.map((entry) => entry.name)).toEqual(['Chrome', 'Safari']);
    expect(series[0]?.timestamps).toEqual([0, HOUR, 2 * HOUR, 3 * HOUR]);
    expect(series[0]?.labels).toEqual({ 'browser.name': 'Chrome' });
    expect(new Set(series.map((entry) => entry.id)).size).toBe(2);
  });

  it('count nothing in a bar that has no row, as a count does', () => {
    const series = chartSeries(counted, { ...inputs, measures: [COUNT] });

    expect(series[0]?.data).toEqual([5, 3, 0, 2]);
    expect(series[1]?.data).toEqual([0, 1, 0, 0]);
  });

  it('leave a gap in a bar that has no row, as a percentile has no value there', () => {
    const slow = answer(
      [
        { group: ['Chrome'], startTime: 0, values: [2_400_000_000] },
        { group: ['Chrome'], startTime: 2 * HOUR, values: [3_100_000_000] },
      ],
      LCP,
    );
    const series = chartSeries(slow, { ...inputs, measures: [{ fn: 'p75', field: 'view.performance.lcp.timestamp' }] });

    expect(series[0]?.data).toEqual([2_400_000_000, null, 3_100_000_000, null]);
    expect(series[0]?.unit).toBe('ns');
  });

  it('keep a value the reader may not see as a gap, not as nothing', () => {
    const masked = answer([{ group: ['Chrome'], startTime: 0, values: ['***'] }], COUNTS);

    expect(chartSeries(masked, { ...inputs, measures: [COUNT] })[0]?.data).toEqual([null, 0, 0, 0]);
  });

  it('are one line named for the number when nothing is grouped', () => {
    const whole = answer([{ group: [], startTime: 0, values: [4] }], COUNTS, []);
    const series = chartSeries(whole, { ...inputs, groups: [], measures: [COUNT] });

    expect(series).toHaveLength(1);
    expect(series[0]?.name).toBe('count of views');
    expect(series[0]?.labels).toBeUndefined();
  });

  it('name a group the rows have no value in, and offer no label to filter by', () => {
    const none = answer([{ group: [null], startTime: 0, values: [1] }], COUNTS);
    const series = chartSeries(none, { ...inputs, measures: [COUNT] });

    expect(series[0]?.name).toBe('(none)');
    expect(series[0]?.labels).toBeUndefined();
  });

  it('count the groups they stand for', () => {
    expect(seriesGroups(counted)).toBe(2);
  });
});

describe('where a vital turns from good to poor', () => {
  const labels = { good: 'Good', poor: 'Poor' };

  it('is marked for a percentile of its field, in the unit the vital is drawn in', () => {
    expect(vitalThresholds({ fn: 'p75', field: 'view.performance.lcp.timestamp' }, labels).map((line) => line.value)).toEqual([2.5, 4]);
    expect(vitalThresholds({ fn: 'p95', field: 'view.performance.inp.duration' }, labels).map((line) => line.value)).toEqual([200, 500]);
    expect(vitalThresholds({ fn: 'avg', field: 'view.performance.cls.score' }, labels).map((line) => line.value)).toEqual([0.1, 0.25]);
  });

  it('is drawn in seconds for the load of a page, in milliseconds for a reaction, and as it is for a score', () => {
    expect(vitalRescale({ fn: 'p75', field: 'view.performance.lcp.timestamp' })).toEqual({ divisor: 1_000_000_000, unit: 's' });
    expect(vitalRescale({ fn: 'p75', field: 'view.performance.inp.duration' })).toEqual({ divisor: 1_000_000, unit: 'ms' });
    expect(vitalRescale({ fn: 'p75', field: 'view.performance.cls.score' })).toBeNull();
    expect(vitalRescale({ fn: 'sum', field: 'view.performance.lcp.timestamp' })).toBeNull();
    expect(vitalRescale({ fn: 'p75', field: 'view.loading_time' })).toBeNull();
  });

  it('is the unit the line of a chart is drawn in', () => {
    const slow = answer([{ group: ['Chrome'], startTime: 0, values: [2_400_000_000] }], LCP);
    const series = chartSeries(slow, { ...inputs, measures: [{ fn: 'p75', field: 'view.performance.lcp.timestamp' }], rescale: { divisor: 1_000_000_000, unit: 's' } });

    expect(series[0]?.data).toEqual([2.4, null, null, null]);
    expect(series[0]?.unit).toBe('s');
  });

  it('is not marked for what is not a level of a vital', () => {
    expect(vitalThresholds({ fn: 'count', field: 'view.performance.lcp.timestamp' }, labels)).toEqual([]);
    expect(vitalThresholds({ fn: 'sum', field: 'view.performance.lcp.timestamp' }, labels)).toEqual([]);
    expect(vitalThresholds({ fn: 'p75', field: 'view.loading_time' }, labels)).toEqual([]);
    expect(vitalThresholds(COUNT, labels)).toEqual([]);
  });
});

describe('a number of an answer', () => {
  it('is told in its unit', () => {
    expect(formatMeasure(1_500_000_000, 'ns', 'en')).toBe('1.5 s');
    expect(formatMeasure(2_048, 'bytes', 'en')).toBe('2.0 KB');
    expect(formatMeasure(12.5, 'percent', 'en')).toBe('12.5%');
    expect(formatMeasure(0.1234, 'score', 'en')).toBe('0.123');
    expect(formatMeasure(48120, 'count', 'en')).toBe('48,120');
    expect(formatMeasure(1.23456, 'none', 'en')).toBe('1.23');
  });

  it('is a dash where there is none, and the mask where the reader may not see it', () => {
    expect(formatMeasure(null, 'count', 'en')).toBe('—');
    expect(formatMeasure(undefined, 'ns', 'en')).toBe('—');
    expect(formatMeasure('***', 'ns', 'en')).toBe('***');
  });
});

describe('what a group is called', () => {
  it('is a country by its name, any other value as it is, and a missing one by the word for none', () => {
    expect(groupLabel('geo.country_iso_code', 'US', 'en', '(none)')).toBe('United States');
    expect(groupLabel('browser.name', 'Chrome', 'en', '(none)')).toBe('Chrome');
    expect(groupLabel('browser.name', null, 'en', '(none)')).toBe('(none)');
    expect(groupLabel('browser.name', '', 'en', '(none)')).toBe('(none)');
    expect(groupsLabel(['browser.name', 'os.name'], ['Chrome', null], 'en', '(none)')).toBe('Chrome · (none)');
  });
});

describe('a ranking and a map', () => {
  it('keep the order the server answered in', () => {
    const ranked = rankedRows(
      answer(
        [
          { group: ['b'], values: [2] },
          { group: ['a'], values: [9] },
        ],
        COUNTS,
      ),
    );

    expect(ranked.map((row) => row.group[0])).toEqual(['b', 'a']);
    expect(new Set(ranked.map((row) => row.key)).size).toBe(2);
  });

  it('draw the countries that have a number: a row with no country has no place', () => {
    const countries = countryValues(
      answer(
        [
          { group: ['US'], values: [10] },
          { group: [null], values: [5] },
          { group: ['DE'], values: [0] },
          { group: ['FR'], values: ['***'] },
          { group: ['BR'], values: [3] },
        ],
        COUNTS,
        ['geo.country_iso_code'],
      ),
    );

    expect(countries).toEqual([
      { code: 'US', value: 10 },
      { code: 'BR', value: 3 },
    ]);
  });
});

describe('a length of time told to a reader', () => {
  it('is a range in the unit it is counted in', () => {
    expect(spanText(24 * HOUR, 'en')).toBe('24 hours');
    expect(spanText(90 * MINUTE, 'en')).toBe('90 minutes');
    expect(spanText(3 * HOUR, 'en')).toBe('3 hours');
    expect(spanText(7 * 24 * HOUR, 'en')).toBe('7 days');
    expect(spanText(30_000_000, 'en')).toBe('30 seconds');
  });

  it('is a bar in the largest unit it is a whole number of', () => {
    expect(bucketText(HOUR, 'en')).toBe('1 hour');
    expect(bucketText(5 * MINUTE, 'en')).toBe('5 minutes');
    expect(bucketText(10_000_000, 'en')).toBe('10 seconds');
    expect(bucketText(24 * HOUR, 'en')).toBe('1 day');
    expect(bucketText(90 * MINUTE, 'en')).toBe('90 minutes');
  });
});
