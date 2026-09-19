import { describe, expect, it } from 'vitest';

import {
  formatLookback,
  formatRelativeMicros,
  formatRunDuration,
  formatSchedule,
  parsePipelineDetailTab,
  pipelineHealth,
  pipelineSuccessRate,
  summarizePipelineRuns,
} from '../presentation';

describe('pipeline presentation', () => {
  it('derives health without inventing a successful run', () => {
    expect(pipelineHealth({ id: '1', name: 'a', enabled: false })).toBe('paused');
    expect(pipelineHealth({ id: '2', name: 'b', last_run_state: 'failed' })).toBe('error');
    expect(pipelineHealth({ id: '3', name: 'c', last_run_state: 'succeeded' })).toBe('healthy');
    expect(pipelineHealth({ id: '4', name: 'd', last_run_at_micros: 123 })).toBe('unknown');
    expect(pipelineHealth({ id: '5', name: 'e' })).toBe('never');
  });

  it('calculates the real 24-hour success rate', () => {
    expect(
      pipelineSuccessRate({
        id: '1',
        name: 'a',
        runs_24h: 10,
        succeeded_runs_24h: 8,
        failed_runs_24h: 2,
      }),
    ).toBe(80);
    expect(pipelineSuccessRate({ id: '2', name: 'b', runs_24h: 0 })).toBeNull();
  });

  it('humanizes schedules, lookback windows, relative time, and duration', () => {
    expect(formatSchedule('every:5m', 'zh-CN')).toBe('每 5 分钟');
    expect(formatSchedule('every:1h', 'en-US')).toBe('Every 1 hour');
    expect(formatLookback(900, 'zh-CN')).toBe('15 分钟');
    expect(formatRelativeMicros(1_699_999_880_000_000, 'zh-CN', 1_700_000_000_000)).toBe(
      '2分钟前',
    );
    expect(formatRunDuration(1_000_000, 2_800_000)).toBe('1.8 s');
  });

  it('normalizes detail tabs and summarizes the latest 24-hour runs', () => {
    expect(parsePipelineDetailTab('configuration')).toBe('configuration');
    expect(parsePipelineDetailTab('invalid')).toBe('overview');

    const nowMicros = 2_000_000_000_000_000;
    const summary = summarizePipelineRuns(
      [
        {
          id: 'new-success',
          pipeline_id: 'pipeline',
          state: 'succeeded',
          started_at_micros: nowMicros - 60_000_000,
          finished_at_micros: nowMicros - 58_000_000,
          scanned_rows: 80,
          error: null,
        },
        {
          id: 'new-failure',
          pipeline_id: 'pipeline',
          state: 'failed',
          started_at_micros: nowMicros - 120_000_000,
          finished_at_micros: nowMicros - 119_000_000,
          scanned_rows: 20,
          error: 'failed',
        },
        {
          id: 'old-success',
          pipeline_id: 'pipeline',
          state: 'succeeded',
          started_at_micros: nowMicros - 25 * 3_600_000_000,
          finished_at_micros: nowMicros - 25 * 3_600_000_000 + 1_000_000,
          scanned_rows: 999,
          error: null,
        },
      ],
      nowMicros,
    );

    expect(summary.lastRun?.id).toBe('new-success');
    expect(summary.runs24h).toHaveLength(2);
    expect(summary.successRate).toBe(50);
    expect(summary.processedRows).toBe(100);
    expect(summary.averageDuration).toBe(1500);
    expect(summary.completedRuns).toBe(2);
  });
});
