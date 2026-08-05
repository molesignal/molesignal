import { describe, expect, it } from 'vitest';

import { heatmapColor, stableValueKey, visualizationColors } from './colors';
import {
  calculationOption,
  finiteNumbers,
  reduceNumericValues,
} from './reduction';
import { normalizeValueRange, valueRatio, zeroInclusiveRange } from './range';
import {
  buildThresholdIntervals,
  resolveThresholdColor,
  thresholdMarkerValues,
} from './thresholds';
import {
  medianPositiveStep,
  normalizedTimelinePositions,
  normalizeTimestamp,
} from './time';

describe('shared visualization models', () => {
  it('filters non-finite samples and applies every reduction alias', () => {
    const values = [2, null, Number.NaN, 6, Infinity, 4];
    expect(finiteNumbers(values)).toEqual([2, 6, 4]);
    expect(reduceNumericValues(values, 'last')).toBe(4);
    expect(reduceNumericValues(values, 'min')).toBe(2);
    expect(reduceNumericValues(values, 'max')).toBe(6);
    expect(reduceNumericValues(values, 'mean')).toBe(4);
    expect(reduceNumericValues(values, 'avg')).toBe(4);
    expect(reduceNumericValues(values, 'sum')).toBe(12);
    expect(calculationOption('unknown')).toBe('last');
  });

  it('normalizes configured and data domains without zero-width output', () => {
    expect(normalizeValueRange(100, 0, 50)).toEqual({ min: 0, max: 100 });
    expect(normalizeValueRange(5, 5, 5)).toEqual({ min: 4, max: 6 });
    expect(zeroInclusiveRange([4, 8])).toEqual({ min: 0, max: 8 });
    expect(zeroInclusiveRange([-8, -4])).toEqual({ min: -8, max: 0 });
    expect(valueRatio(200, { min: 0, max: 100 })).toBe(1);
  });

  it('converts percentage thresholds into absolute intervals and markers', () => {
    const thresholds = {
      mode: 'percentage' as const,
      steps: [
        { value: null, color: 'green' },
        { value: 75, color: 'red', label: 'High' },
      ],
    };
    const range = { min: 20, max: 120 };
    expect(buildThresholdIntervals(thresholds, range)).toEqual([
      { start: 20, end: 95, color: 'green' },
      { start: 95, end: 120, color: 'red', label: 'High' },
    ]);
    expect(thresholdMarkerValues(thresholds, range)).toEqual([95]);
    expect(resolveThresholdColor(95, thresholds, range)).toBe('red');
  });

  it('normalizes second, millisecond, microsecond, and ISO timestamps', () => {
    expect(normalizeTimestamp(1_700_000_000)).toBe(1_700_000_000);
    expect(normalizeTimestamp(1_700_000_000_000)).toBe(1_700_000_000);
    expect(normalizeTimestamp(1_700_000_000_000_000)).toBe(1_700_000_000);
    expect(normalizeTimestamp('2023-11-14T22:13:20.000Z')).toBe(1_700_000_000);
    expect(medianPositiveStep([0, 10, 40, 50])).toBe(10);
  });

  it('falls back to ordered positions for incomplete or decreasing time data', () => {
    expect(normalizedTimelinePositions([10, 20, 50], 3)).toEqual({
      values: [10, 20, 50],
      usesTime: true,
    });
    expect(normalizedTimelinePositions([10, null, 50], 3)).toEqual({
      values: [0, 1, 2],
      usesTime: false,
    });
    expect(normalizedTimelinePositions([10, 5], 2).usesTime).toBe(false);
  });

  it('assigns deterministic token colors and stable state identities', () => {
    const colors = visualizationColors(['cpu', 'memory', 'disk']);
    expect(new Set(colors).size).toBe(3);
    expect(visualizationColors(['cpu', 'memory', 'disk'])).toEqual(colors);
    expect(heatmapColor('greens')).toBe('var(--green)');
    expect(stableValueKey({ state: 'ok' })).toBe('{"state":"ok"}');
  });
});
