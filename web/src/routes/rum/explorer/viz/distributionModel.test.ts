import { describe, expect, it } from 'vitest';

import type { RumDistribution } from '@/api/rum';

import { markerPlaces } from './distributionModel';

const answer = (patch: Partial<RumDistribution> = {}): RumDistribution => ({
  field: 'view.loading_time',
  unit: 'ns',
  rows: 10,
  min: 0,
  max: 500,
  p50: 5,
  p75: 15,
  p95: 25,
  p99: 40,
  bars: [
    { from: 0, to: 10, rows: 5 },
    { from: 10, to: 20, rows: 3 },
    { from: 20, to: 30, rows: 1 },
    { from: 30, to: 500, rows: 1 },
  ],
  ...patch,
});

describe('where the percentiles fall on the bars', () => {
  it('is along the bar that holds the value', () => {
    expect(markerPlaces(answer(), ['p50', 'p75', 'p95'])).toEqual([
      { name: 'p50', place: 0.125, line: 0 },
      { name: 'p75', place: 0.375, line: 0 },
      { name: 'p95', place: 0.625, line: 0 },
    ]);
  });

  it('stays on the chart for a value past the bars', () => {
    expect(markerPlaces(answer({ p99: 900 }), ['p99'])).toEqual([{ name: 'p99', place: 1, line: 0 }]);
  });

  it('puts the names of marks that stand close on lines of their own', () => {
    const marks = markerPlaces(answer({ p95: 40, p99: 41 }), ['p50', 'p95', 'p99']);

    expect(marks.map((mark) => mark.line)).toEqual([0, 0, 1]);
  });

  it('marks nothing on a single bar, nor a percentile that has no value', () => {
    expect(markerPlaces(answer({ bars: [{ from: 4, to: 4, rows: 10 }] }), ['p50'])).toEqual([]);
    expect(markerPlaces(answer({ p50: null }), ['p50'])).toEqual([]);
  });
});
