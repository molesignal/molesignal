import { describe, expect, it } from 'vitest';

import { resolveSystemStatus } from '@/shell/SystemStatusIndicator';

describe('resolveSystemStatus', () => {
  it('maps a healthy response to the green state', () => {
    expect(resolveSystemStatus({ status: 'ok' }, false)).toBe('healthy');
  });

  it('maps an explicit degraded response to the yellow state', () => {
    expect(resolveSystemStatus({ status: 'degraded' }, false)).toBe(
      'degraded',
    );
  });

  it('maps unavailable or failed health checks to the red state', () => {
    expect(resolveSystemStatus(undefined, false)).toBe('disconnected');
    expect(resolveSystemStatus({ status: 'ok' }, true)).toBe('disconnected');
  });
});
