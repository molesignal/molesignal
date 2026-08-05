import { describe, expect, it } from 'vitest';

import {
  timeSeriesColor,
  timeSeriesColors,
} from '@/viz/timeseries/colors';

describe('time-series colours', () => {
  it('keeps the single-series mapping deterministic', () => {
    expect(timeSeriesColor('checkout')).toBe(timeSeriesColor('checkout'));
  });

  it('avoids palette collisions for up to eight visible series', () => {
    const colors = timeSeriesColors(
      Array.from({ length: 8 }, (_, index) => `series-${index}`),
    );
    expect(new Set(colors)).toHaveLength(8);
  });
});
