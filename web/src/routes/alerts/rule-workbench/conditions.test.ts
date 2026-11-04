import { describe, expect, it } from 'vitest';

import type { AlertRule, SeverityThreshold } from '@/types/alerting';

import { conditionsOf, conditionsPayload, recoveryLineInvalid } from './conditions';

const band = (operator: SeverityThreshold['operator'], threshold: number): SeverityThreshold => ({
  severity: 'warning',
  operator,
  threshold,
  forPeriods: 1,
});

describe('recovery and missing data', () => {
  it('leaves out what a rule would do anyway', () => {
    const fresh = conditionsOf(undefined, 60);

    expect(fresh).toEqual({ recoveryThreshold: '', recoveryPeriods: 1, noDataAction: 'resolve', noDataPeriods: 5 });
    expect(conditionsPayload(fresh, 'scheduled')).toEqual({ recovery: null, noData: null });
    // An anomaly rule keeps its state without a value unless told.
    expect(conditionsPayload({ ...fresh, noDataAction: 'keep' }, 'anomaly').noData).toBeNull();
    expect(conditionsPayload({ ...fresh, noDataAction: 'resolve' }, 'anomaly').noData).toEqual({
      action: 'resolve',
      forPeriods: 0,
    });
  });

  it('saves a recovery line, the rounds to recover, and how long to wait for no data', () => {
    const payload = conditionsPayload(
      { recoveryThreshold: ' 80 ', recoveryPeriods: 3, noDataAction: 'alert', noDataPeriods: 10 },
      'scheduled',
    );

    expect(payload).toEqual({
      recovery: { threshold: 80, forPeriods: 3 },
      noData: { action: 'alert', forPeriods: 10 },
    });
  });

  it('reads them back from a saved rule', () => {
    const rule = {
      kind: 'scheduled',
      trigger: {
        operator: 'gt',
        threshold: 90,
        forPeriods: 1,
        silenceSecs: 0,
        recovery: { threshold: 80, forPeriods: 2 },
        noData: { action: 'alert', forPeriods: 4 },
      },
    } as AlertRule;

    expect(conditionsOf(rule, 60)).toEqual({
      recoveryThreshold: '80',
      recoveryPeriods: 2,
      noDataAction: 'alert',
      noDataPeriods: 4,
    });
  });

  it('keeps the recovery line on the healthy side of every band', () => {
    expect(recoveryLineInvalid([band('gt', 90)], '')).toBe(false);
    expect(recoveryLineInvalid([band('gt', 90)], '80')).toBe(false);
    expect(recoveryLineInvalid([band('gt', 90)], '95')).toBe(true);
    expect(recoveryLineInvalid([band('lt', 10)], '20')).toBe(false);
    expect(recoveryLineInvalid([band('gte', 80), band('gt', 95)], '85')).toBe(true);
    expect(recoveryLineInvalid([band('gt', 80), band('lt', 5)], '50')).toBe(true);
    expect(recoveryLineInvalid([band('eq', 1)], '1')).toBe(true);
  });
});
