import { describe, expect, it } from 'vitest';

import { sparklineBars, sparklineGeometry } from './trendGeometry';

describe('sparklineGeometry', () => {
  it('needs two samples to have a shape', () => {
    expect(sparklineGeometry([])).toBeNull();
    expect(sparklineGeometry([4])).toBeNull();
    expect(sparklineGeometry([null, 4, undefined])).toBeNull();
  });

  it('spans the box from the first sample to the last, highest at the top', () => {
    const geometry = sparklineGeometry([0, 10, 5])!;

    expect(geometry.line).toBe('M 0,30 L 50,2 L 100,16');
    expect(geometry.end).toEqual({ x: 100, y: 50 });
    // The area closes down to the baseline under the line.
    expect(geometry.area.endsWith('L 100,32 L 0,32 Z')).toBe(true);
  });

  it('joins across gaps instead of drawing them as zero', () => {
    expect(sparklineGeometry([2, null, 4])?.line).toBe(sparklineGeometry([2, 4])?.line);
  });

  it('draws nothing happening on the baseline and any other flat level in the middle', () => {
    expect(sparklineGeometry([0, 0, 0])?.line).toBe('M 0,30 L 50,30 L 100,30');
    expect(sparklineGeometry([7, 7])?.line).toBe('M 0,16 L 100,16');
  });

  it('can scale from zero, so a high steady level does not look like a swing', () => {
    const tight = sparklineGeometry([98, 100])!;
    const fromZero = sparklineGeometry([98, 100], { zeroBased: true })!;

    expect(tight.line).toBe('M 0,30 L 100,2');
    expect(fromZero.line).toBe('M 0,2.56 L 100,2');
  });
});

describe('sparklineBars', () => {
  it('gives every sample a bar that grows from the baseline', () => {
    const bars = sparklineBars([0, 5, 10]);

    expect(bars).toHaveLength(3);
    expect(bars.map((bar) => bar.height)).toEqual([0, 16, 32]);
    expect(bars.map((bar) => bar.y + bar.height)).toEqual([32, 32, 32]);
    // Bars sit in equal slots and do not touch.
    expect(bars[1]!.x).toBeGreaterThan(bars[0]!.x + bars[0]!.width);
  });

  it('keeps a small count visible beside a large one, and a gap empty', () => {
    const bars = sparklineBars([1, 1000, null]);

    expect(bars[0]!.height).toBe(1.5);
    expect(bars[2]!.height).toBe(0);
  });

  it('draws nothing for a window with nothing in it', () => {
    expect(sparklineBars([])).toEqual([]);
    expect(sparklineBars([0, 0]).every((bar) => bar.height === 0)).toBe(true);
  });
});
