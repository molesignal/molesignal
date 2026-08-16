import { describe, expect, it } from 'vitest';

import { widenTimeWindow } from './timeRangeRecovery';

describe('widenTimeWindow', () => {
  it('expands relative ranges into a deterministic absolute window', () => {
    expect(
      widenTimeWindow(
        { mode: 'relative', from: 'now-1h', to: 'now' },
        4,
        new Date('2026-08-14T12:00:00.000Z'),
      ),
    ).toEqual({
      mode: 'absolute',
      from: '2026-08-14T08:00:00.000Z',
      to: '2026-08-14T12:00:00.000Z',
    });
  });
});
