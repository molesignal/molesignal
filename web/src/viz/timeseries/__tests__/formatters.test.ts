import { describe, expect, it } from 'vitest';

import {
  formatTimeSeriesAxisTimestamp,
  formatTimeSeriesAxisValues,
  formatTimeSeriesTimestamp,
  formatTimeSeriesValue,
} from '@/viz/timeseries/formatters';

const EPOCH_SECONDS = Date.UTC(2026, 6, 25, 2, 26, 13, 455) / 1000;

describe('time-series timestamp formatting', () => {
  it('uses ISO numeric date-time text independent of the UI locale', () => {
    expect(formatTimeSeriesTimestamp(EPOCH_SECONDS, true, 'Asia/Shanghai')).toBe(
      '2026-07-25 10:26:13.455',
    );
    expect(formatTimeSeriesTimestamp(EPOCH_SECONDS, false, 'Asia/Shanghai')).toBe(
      '07-25 10:26:13',
    );
  });

  it('uses locale-independent ISO fragments for every axis span', () => {
    expect(
      formatTimeSeriesAxisTimestamp(EPOCH_SECONDS, 40 * 86_400, 'Asia/Shanghai'),
    ).toBe('2026-07-25');
    expect(
      formatTimeSeriesAxisTimestamp(EPOCH_SECONDS, 8 * 86_400, 'Asia/Shanghai'),
    ).toBe('07-25');
    expect(
      formatTimeSeriesAxisTimestamp(EPOCH_SECONDS, 7 * 86_400, 'Asia/Shanghai'),
    ).toBe('07-25 10:26');
    expect(
      formatTimeSeriesAxisTimestamp(EPOCH_SECONDS, 2 * 3600, 'Asia/Shanghai'),
    ).toBe('10:26');
    expect(
      formatTimeSeriesAxisTimestamp(EPOCH_SECONDS, 30 * 60, 'Asia/Shanghai'),
    ).toBe('10:26:13');
  });

  it('writes seconds only when the ticks fall within a minute of each other', () => {
    expect(formatTimeSeriesAxisTimestamp(EPOCH_SECONDS, 3600, 'Asia/Shanghai', 15 * 60)).toBe('10:26');
    expect(formatTimeSeriesAxisTimestamp(EPOCH_SECONDS, 300, 'Asia/Shanghai', 30)).toBe('10:26:13');
  });
});

describe('time-series value formatting', () => {
  it('picks the unit a duration reads best in', () => {
    expect(formatTimeSeriesValue(400, 'ms')).toBe('400 ms');
    expect(formatTimeSeriesValue(2_500, 'ms')).toBe('2.5 s');
    expect(formatTimeSeriesValue(1_500_000, 'ns')).toBe('1.5 ms');
  });

  it('keeps the unit of a zero: the first tick of an axis in milliseconds is not nanoseconds', () => {
    expect(formatTimeSeriesValue(0, 'ms')).toBe('0 ms');
    expect(formatTimeSeriesValue(0, 'ns')).toBe('0 ns');
    expect(formatTimeSeriesValue(0, 's')).toBe('0 s');
    expect(formatTimeSeriesValue(0, 'μs')).toBe('0 μs');
  });
});

describe('time-series axis figures', () => {
  it('writes a duration axis in the unit of its largest tick, once, and its zero bare', () => {
    expect(formatTimeSeriesAxisValues([0, 250_000, 500_000, 750_000], 'μs')).toEqual(['0', '250', '500', '750 ms']);
    expect(formatTimeSeriesAxisValues([0, 500, 1_000, 1_500], 'us')).toEqual(['0', '0.5', '1', '1.5 ms']);
    expect(formatTimeSeriesAxisValues([0, 1_000, 2_000], 'ms')).toEqual(['0', '1', '2 s']);
  });

  it('writes a rate, a percentage and a size once, on the top tick', () => {
    expect(formatTimeSeriesAxisValues([0, 0.5, 1, 1.5], 'req/s')).toEqual(['0', '0.5', '1', '1.5 req/s']);
    expect(formatTimeSeriesAxisValues([0, 1, 2], '%')).toEqual(['0', '1', '2%']);
    expect(formatTimeSeriesAxisValues([0, 512, 1_024, 1_536], 'bytes')).toEqual(['0', '0.5', '1', '1.5 KiB']);
  });

  it('keeps one magnitude for a count, so 500 and 1K do not mix', () => {
    expect(formatTimeSeriesAxisValues([0, 500, 1_000, 1_500])).toEqual(['0', '0.5', '1', '1.5K']);
    expect(formatTimeSeriesAxisValues([0, 20, 40, 60])).toEqual(['0', '20', '40', '60']);
  });
});
