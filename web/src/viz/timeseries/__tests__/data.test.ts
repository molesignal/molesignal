import { describe, expect, it } from 'vitest';

import {
  calculateTimeSeriesStats,
  downsampleAlignedData,
  prepareTimeSeriesData,
  toInputTimestamp,
} from '@/viz/timeseries/data';

describe('time-series data preparation', () => {
  it('normalizes microseconds, sorts points, deduplicates timestamps and preserves nulls', () => {
    const prepared = prepareTimeSeriesData(
      [
        {
          name: 'requests',
          timestamps: [1_700_000_002_000_000, 1_700_000_001_000_000, 1_700_000_002_000_000],
          data: [2, null, 3],
        },
      ],
      [1_700_000_000_000_000, 1_700_000_003_000_000],
      'none',
    );

    expect(prepared.inputTimestampScale).toBe(1 / 1_000_000);
    expect(prepared.rawData[0]).toEqual([1_700_000_001, 1_700_000_002]);
    expect(prepared.rawData[1]).toEqual([null, 3]);
    expect(toInputTimestamp(1_700_000_001, prepared.inputTimestampScale)).toBe(
      1_700_000_001_000_000,
    );
  });

  it('aligns series with different timestamps instead of converting missing values to zero', () => {
    const prepared = prepareTimeSeriesData(
      [
        { name: 'a', timestamps: [1, 3], data: [10, 30] },
        { name: 'b', timestamps: [2, 3], data: [20, 40] },
      ],
      undefined,
      'none',
    );

    expect(prepared.rawData).toEqual([
      [1, 2, 3],
      [10, null, 30],
      [null, 20, 40],
    ]);
  });

  it('builds cumulative and percentage stacks while keeping raw tooltip values', () => {
    const normal = prepareTimeSeriesData(
      [
        { name: 'a', data: [1, 3] },
        { name: 'b', data: [1, 1] },
      ],
      undefined,
      'normal',
    );
    const percent = prepareTimeSeriesData(
      [
        { name: 'a', data: [1, 3] },
        { name: 'b', data: [1, 1] },
      ],
      undefined,
      'percent',
    );

    expect(normal.data).toEqual([[0, 1], [1, 3], [2, 4]]);
    expect(normal.rawData).toEqual([[0, 1], [1, 3], [1, 1]]);
    expect(percent.data[1]).toEqual([50, 75]);
    expect(percent.data[2]).toEqual([100, 100]);
  });

  it('retains endpoints and a sharp spike during downsampling', () => {
    const xs = Array.from({ length: 1_000 }, (_, index) => index);
    const values = xs.map((index) => (index === 503 ? 10_000 : Math.sin(index / 20)));
    const reduced = downsampleAlignedData([xs, values], 80);

    expect(reduced[0]?.[0]).toBe(0);
    expect(reduced[0]?.at(-1)).toBe(999);
    expect(reduced[1]).toContain(10_000);
    expect(reduced[0]?.length).toBeLessThanOrEqual(80);
  });

  it('computes legend statistics from finite values only', () => {
    expect(calculateTimeSeriesStats([null, 2, Number.NaN, 4])).toEqual({
      last: 4,
      min: 2,
      max: 4,
      mean: 3,
      sum: 6,
      count: 2,
    });
  });
});
