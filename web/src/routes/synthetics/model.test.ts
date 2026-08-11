import { describe, expect, it } from 'vitest';

import type { SyntheticResult } from '@/api/synthetics';

import { operationalResults, percentile, resultDurationMicros, successRate } from './model';

function result(
  id: string,
  outcome: SyntheticResult['outcome'],
  totalMicros?: number,
  isTest = false,
): SyntheticResult {
  return {
    id,
    organization_id: 'org-a',
    monitor_id: 'monitor-a',
    monitor_revision_id: 'revision-a',
    location_id: 'location-a',
    task_id: `task-${id}`,
    is_test: isTest,
    scheduled_at: 1,
    started_at: 10,
    finished_at: 30,
    received_at: 40,
    outcome,
    attempts: totalMicros === undefined
      ? []
      : [{
          number: 1,
          started_at: 10,
          finished_at: 30,
          outcome,
          timing: { total_micros: totalMicros },
          metadata: {},
        }],
    assertions: [],
    secret_versions: {},
    protocol_version: 1,
    metadata: {},
  };
}

describe('synthetics result model', () => {
  it('excludes unknown and skipped executions from availability', () => {
    expect(
      successRate([
        result('healthy', 'healthy'),
        result('failing', 'failing'),
        result('unknown', 'unknown'),
        result('skipped', 'skipped'),
      ]),
    ).toBe(0.5);
  });

  it('excludes test runs from operational metrics', () => {
    const scheduled = result('scheduled', 'healthy');
    const testRun = result('test', 'failing', undefined, true);

    expect(operationalResults([testRun, scheduled])).toEqual([scheduled]);
    expect(successRate([testRun, scheduled])).toBe(1);
  });

  it('uses attempt timing and falls back to execution duration', () => {
    expect(resultDurationMicros(result('attempt', 'healthy', 1_250))).toBe(1_250);
    expect(resultDurationMicros(result('fallback', 'healthy'))).toBe(20);
  });

  it('calculates a nearest-rank percentile without mutating the input', () => {
    const values = [400, 100, 300, 200];
    expect(percentile(values, 0.95)).toBe(400);
    expect(values).toEqual([400, 100, 300, 200]);
  });
});
