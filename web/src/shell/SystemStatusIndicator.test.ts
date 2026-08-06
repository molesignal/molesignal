import { describe, expect, it } from 'vitest';

import { DegradedSystemHealthError } from '@/api/health';
import {
  nextSystemHealthCheckDelay,
  resolveSystemStatus,
} from '@/shell/SystemStatusIndicator';

describe('resolveSystemStatus', () => {
  it('maps a healthy response to the green state', () => {
    expect(resolveSystemStatus({ status: 'ok' }, null)).toBe('healthy');
  });

  it('maps an explicit degraded response to the yellow state', () => {
    expect(resolveSystemStatus({ status: 'degraded' }, null)).toBe(
      'degraded',
    );
    expect(
      resolveSystemStatus(
        undefined,
        new DegradedSystemHealthError({ status: 'degraded' }),
      ),
    ).toBe('degraded');
  });

  it('maps unavailable or failed health checks to the red state', () => {
    expect(resolveSystemStatus(undefined, null)).toBe('disconnected');
    expect(resolveSystemStatus({ status: 'ok' }, new Error('offline'))).toBe(
      'disconnected',
    );
  });
});

describe('system health polling schedule', () => {
  it('jitters successful checks uniformly across 30 seconds plus or minus 5 seconds', () => {
    expect(nextSystemHealthCheckDelay(0, () => 0)).toBe(25_000);
    expect(nextSystemHealthCheckDelay(0, () => 0.5)).toBe(30_000);
    expect(nextSystemHealthCheckDelay(0, () => 1)).toBe(35_000);
  });

  it('backs off consecutive failures and caps retries at 60 seconds', () => {
    expect(
      [1, 2, 3, 4, 5, 6, 20].map((failureCount) =>
        nextSystemHealthCheckDelay(failureCount),
      ),
    ).toEqual([5_000, 10_000, 20_000, 30_000, 60_000, 60_000, 60_000]);
  });

  it('returns to the jittered success cadence after recovery', () => {
    expect(nextSystemHealthCheckDelay(4)).toBe(30_000);
    expect(nextSystemHealthCheckDelay(0, () => 0.25)).toBe(27_500);
  });
});
