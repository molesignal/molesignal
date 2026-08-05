import { describe, expect, it } from 'vitest';

import { downsampleSeries } from '@/viz/timeseries/lttb';

describe('LTTB downsampling', () => {
  it('returns the input untouched when below target', () => {
    const xs = [1, 2, 3, 4, 5];
    const ys = [10, 20, 30, 40, 50];
    const [xOut, yOut] = downsampleSeries([xs, ys], 10);
    expect(xOut).toEqual(xs);
    expect(yOut).toEqual(ys);
  });

  it('reduces sample count to the requested target (±1)', () => {
    const n = 1000;
    const xs = Array.from({ length: n }, (_, i) => i);
    const ys = xs.map((i) => Math.sin(i / 5));
    const out = downsampleSeries([xs, ys], 100);
    const xOut = out[0]!;
    expect(xOut.length).toBeGreaterThan(50);
    expect(xOut.length).toBeLessThanOrEqual(100);
  });

  it('preserves first and last sample', () => {
    const xs = Array.from({ length: 100 }, (_, i) => i);
    const ys = xs.map((i) => i * 2);
    const out = downsampleSeries([xs, ys], 10);
    const xOut = out[0]!;
    const yOut = out[1]!;
    expect(xOut[0]).toBe(xs[0]);
    expect(xOut[xOut.length - 1]).toBe(xs[xs.length - 1]);
    expect(yOut[0]).toBe(ys[0]);
    expect(yOut[yOut.length - 1]).toBe(ys[ys.length - 1]);
  });
});
