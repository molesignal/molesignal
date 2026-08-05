import { describe, expect, it } from 'vitest';

import {
  calculatePanRange,
  resolveInteractionRange,
} from '@/viz/timeseries/brush';

describe('Grafana-style time-range interactions', () => {
  it('keeps the visible duration stable while the x-axis pans', () => {
    const panned = calculatePanRange(1_000, 1_100, -200, 1_000);

    expect(panned).toEqual({ from: 1_020, to: 1_120 });
    expect(panned.to - panned.from).toBe(100);
  });

  it('holds a live pan range while the external range is still stale', () => {
    const interaction = {
      from: 1_020,
      to: 1_120,
      isTimeRangePending: true,
    };
    const resolved = resolveInteractionRange([1_000, 1_100], interaction);

    expect(resolved.range).toEqual([1_020, 1_120]);
    expect(resolved.interaction).toBe(interaction);
  });

  it('releases the live range only after the external range catches up', () => {
    const resolved = resolveInteractionRange(
      [1_020.0005, 1_120.0005],
      {
        from: 1_020,
        to: 1_120,
        isTimeRangePending: true,
      },
    );

    expect(resolved.range).toEqual([1_020.0005, 1_120.0005]);
    expect(resolved.interaction).toBeNull();
  });
});
