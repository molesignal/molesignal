import { describe, expect, it } from 'vitest';

import {
  formatLookback,
  formatRelativeMicros,
  formatRunDuration,
  formatSchedule,
  pipelineHealth,
  pipelineSuccessRate,
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
});
