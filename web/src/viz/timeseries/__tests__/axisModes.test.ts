import { describe, expect, it } from 'vitest';

import type uPlot from 'uplot';

import {
  buildScale,
  buildTimeSeriesAxisScale,
  resolveStackedAxisOptions,
} from '@/viz/timeseries/axisModes';
import type { TimeSeriesAxisOptions } from '@/viz/timeseries/types';

describe('axisModes.buildScale', () => {
  it('linear returns a uPlot scale', () => {
    const s = buildScale({ key: 'y', mode: 'linear' });
    expect(s).toBeTruthy();
  });

  it('log returns distr=3 with log base 10', () => {
    const s = buildScale({ key: 'y', mode: 'log' });
    expect(s.distr).toBe(3);
  });

  it('percentile supplies a custom distr', () => {
    const s = buildScale({ key: 'y', mode: 'percentile' }) as unknown as { distr?: number };
    expect(s.distr).toBe(100);
  });

  it('expands an all-zero range upward while preserving the zero baseline', () => {
    expect(resolveRange({ scale: 'linear', softMin: 0 }, 0, 0)).toEqual([
      0,
      1,
    ]);
  });

  it('does not change a non-degenerate configured range', () => {
    expect(resolveRange({ scale: 'linear', softMin: 0 }, 0, 12)).toEqual([
      0,
      12,
    ]);
  });

  it('uses a fixed 0–100 percent axis for percent stacking', () => {
    expect(
      resolveStackedAxisOptions(
        { scale: 'linear', unit: 'req/s', softMin: 0 },
        'percent',
      ),
    ).toEqual({
      scale: 'linear',
      unit: '%',
      softMin: 0,
      min: 0,
      max: 100,
    });
  });
});

function resolveRange(
  axis: TimeSeriesAxisOptions,
  dataMin: number,
  dataMax: number,
): uPlot.Range.MinMax {
  const range = buildTimeSeriesAxisScale(axis).range;
  if (typeof range !== 'function') {
    throw new Error('expected a configured range function');
  }
  return range({} as uPlot, dataMin, dataMax, 'y');
}
