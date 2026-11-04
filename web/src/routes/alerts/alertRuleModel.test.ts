import { describe, expect, it } from 'vitest';

import type { QueryResult } from '@/types/query';

import {
  extractQueryPoints,
  thresholdConflict,
} from './alertRuleModel';

describe('alert rule preview model', () => {
  it('extracts timestamps and numeric values from a query result', () => {
    const result: QueryResult = {
      columns: ['_timestamp', 'service', 'value'],
      rows: [
        [1_720_000_000_000_000, 'api', '0.12'],
        [1_720_000_060_000_000, 'api', 0.18],
      ],
      scannedRows: 2,
      tookMs: 4,
    };

    expect(extractQueryPoints(result)).toEqual([
      { timestamp: 1_720_000_000_000_000, value: 0.12 },
      { timestamp: 1_720_000_060_000_000, value: 0.18 },
    ]);
  });

  it('detects a critical threshold that is easier to hit than warning', () => {
    expect(
      thresholdConflict([
        { severity: 'warning', operator: 'gt', threshold: 0.05, forPeriods: 5 },
        { severity: 'critical', operator: 'gt', threshold: 0.03, forPeriods: 2 },
      ]),
    ).toBe(true);
  });
});
