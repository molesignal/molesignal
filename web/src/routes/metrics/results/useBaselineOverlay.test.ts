import { describe, expect, it } from 'vitest';

import { shifted } from './useBaselineOverlay';

const HOUR_MICROS = 3_600_000_000;

describe('baseline overlay', () => {
  it('moves a moment by microseconds in the unit it is written in', () => {
    expect(shifted(1_795_514_400_000_000, HOUR_MICROS)).toBe(1_795_518_000_000_000);
    expect(shifted(1_795_514_400_000, HOUR_MICROS)).toBe(1_795_518_000_000);
    expect(shifted(1_795_514_400, HOUR_MICROS)).toBe(1_795_518_000);
  });
});
