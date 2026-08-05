import { describe, expect, it } from 'vitest';

import {
  autoRefreshIntervalMilliseconds,
  parseIntervalMilliseconds,
  refreshCadenceFromSettings,
  resolveRefreshIntervalMilliseconds,
} from './policy';

describe('dashboard refresh policy', () => {
  it('uses an adaptive Auto interval instead of a one-second poll', () => {
    expect(autoRefreshIntervalMilliseconds(60 * 60 * 1_000, 1_600)).toBe(
      5_000,
    );
    expect(autoRefreshIntervalMilliseconds(24 * 60 * 60 * 1_000, 1_600)).toBe(
      60_000,
    );
    expect(autoRefreshIntervalMilliseconds(30 * 24 * 60 * 60 * 1_000, 1_600)).toBe(
      1_800_000,
    );
  });

  it('refreshes narrower panels less aggressively for the same range', () => {
    const range = 24 * 60 * 60 * 1_000;
    expect(autoRefreshIntervalMilliseconds(range, 1_600)).toBe(60_000);
    expect(autoRefreshIntervalMilliseconds(range, 600)).toBe(300_000);
  });

  it('maps persisted live mode to Auto and preserves explicit intervals', () => {
    expect(
      refreshCadenceFromSettings({
        enabled: true,
        mode: 'live',
        allowedIntervals: ['off', '30s'],
      }),
    ).toBe('auto');
    expect(
      refreshCadenceFromSettings({
        enabled: true,
        mode: 'interval',
        defaultInterval: '30s',
        allowedIntervals: ['off', '30s'],
      }),
    ).toBe(30_000);
    expect(parseIntervalMilliseconds('5m')).toBe(300_000);
    expect(
      resolveRefreshIntervalMilliseconds(
        false,
        { from: 0, to: 3_600_000_000 },
        1_600,
      ),
    ).toBe(false);
  });
});
