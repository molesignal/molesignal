import { describe, expect, it } from 'vitest';

import {
  buildThresholdIntervals,
  drawRadialArcPath,
  gaugeValueRatio,
  normalizeGaugeRange,
  resolveThresholdColor,
} from './geometry';

describe('radial gauge geometry', () => {
  it('normalizes default, reversed, and equal ranges', () => {
    expect(normalizeGaugeRange(undefined, undefined, 150)).toEqual({
      min: 0,
      max: 150,
    });
    expect(normalizeGaugeRange(100, 0, 50)).toEqual({ min: 0, max: 100 });
    expect(normalizeGaugeRange(0, 0, 0)).toEqual({ min: -1, max: 1 });
    expect(normalizeGaugeRange(50, 50, 50)).toEqual({ min: 45, max: 55 });
  });

  it('clamps only the rendered ratio', () => {
    const range = { min: 0, max: 100 };
    expect(gaugeValueRatio(-10, range)).toBe(0);
    expect(gaugeValueRatio(25, range)).toBe(0.25);
    expect(gaugeValueRatio(120, range)).toBe(1);
    expect(gaugeValueRatio(Number.NaN, range)).toBe(0);
  });

  it('draws deterministic radial SVG paths', () => {
    expect(drawRadialArcPath(0, 90, 10, 20, 20)).toBe(
      'M 20 10 A 10 10 0 0 1 30 20',
    );
    expect(drawRadialArcPath(250, 220, 80, 130, 112)).toContain(
      'A 80 80 0 1 1',
    );
    expect(drawRadialArcPath(0, 0, 10)).toBe('');
    expect(drawRadialArcPath(0, 90, Number.NaN)).toBe('');
  });

  it('builds ordered absolute threshold intervals', () => {
    expect(
      buildThresholdIntervals(
        {
          mode: 'absolute',
          steps: [
            { value: 80, color: 'red', label: 'Critical' },
            { value: null, color: 'green' },
            { value: 40, color: 'yellow' },
          ],
        },
        { min: 0, max: 100 },
      ),
    ).toEqual([
      { start: 0, end: 40, color: 'green' },
      { start: 40, end: 80, color: 'yellow' },
      { start: 80, end: 100, color: 'red', label: 'Critical' },
    ]);
  });

  it('converts percentage thresholds against the normalized range', () => {
    const thresholds = {
      mode: 'percentage' as const,
      steps: [
        { value: null, color: 'green' },
        { value: 80, color: 'red', label: 'High' },
      ],
    };
    const range = { min: 50, max: 150 };

    expect(buildThresholdIntervals(thresholds, range)).toEqual([
      { start: 50, end: 130, color: 'green' },
      { start: 130, end: 150, color: 'red', label: 'High' },
    ]);
    expect(resolveThresholdColor(129, thresholds, range)).toBe('green');
    expect(resolveThresholdColor(130, thresholds, range)).toBe('red');
  });
});
