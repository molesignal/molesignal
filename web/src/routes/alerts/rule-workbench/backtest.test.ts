import { describe, expect, it } from 'vitest';

import type { SeverityThreshold } from '@/types/alerting';

import { backtestRule } from './backtest';

const band = (for_periods: number): SeverityThreshold => ({
  severity: 'warning',
  operator: 'gt',
  threshold: 5,
  forPeriods: for_periods,
});

const times = [10, 20, 30, 40, 50, 60, 70, 80];
const values = [1, 8, 9, 7, 2, 8, 9, 1];

describe('backtestRule', () => {
  it('fires once the line has held for the rounds asked, and says from when', () => {
    const result = backtestRule(values, times, band(2));

    expect(result.firing).toEqual([
      { from: 30, to: 50 },
      { from: 70, to: 80 },
    ]);
    expect(result.pending).toEqual([
      { from: 20, to: 30 },
      { from: 60, to: 70 },
    ]);
    expect(result.nearMisses).toBe(0);
  });

  it('counts what crossed the line and fell back too soon as a near miss', () => {
    const result = backtestRule(values, times, band(3));

    expect(result.firing).toEqual([{ from: 40, to: 50 }]);
    expect(result.nearMisses).toBe(1);
    expect(result.pending).toContainEqual({ from: 60, to: 80 });
  });

  it('fires at once when one round is enough', () => {
    const result = backtestRule([1, 8, 1], [10, 20, 30], band(1));

    expect(result.firing).toEqual([{ from: 20, to: 30 }]);
    expect(result.pending).toEqual([]);
  });

  it('keeps a run still going at the end of the stretch', () => {
    const result = backtestRule([1, 8, 9], [10, 20, 30], band(2));

    expect(result.firing).toEqual([{ from: 30, to: 30 }]);
  });

  it('does not call a run still over the line at the end a near miss', () => {
    const result = backtestRule([1, 8, 9], [10, 20, 30], band(3));

    expect(result.pending).toEqual([{ from: 20, to: 30 }]);
    expect(result.nearMisses).toBe(0);
  });

  it('says nothing about what never crossed the line', () => {
    expect(backtestRule([1, 2, 3], [10, 20, 30], band(1))).toEqual({ pending: [], firing: [], nearMisses: 0 });
  });
});
