import { describe, expect, it } from 'vitest';

import { PROMQL_MAX_STEPS, promqlStepMicros } from './promqlRange';

const HOUR = 3_600_000_000;
const DAY = 24 * HOUR;

describe('promqlStepMicros', () => {
  it('divides the range into a thousand steps when nobody chose one', () => {
    expect(promqlStepMicros(0, HOUR)).toBe(3_600_000);
    expect(promqlStepMicros(0, HOUR, null, 60)).toBe(60_000_000);
  });

  it('keeps a chosen step', () => {
    expect(promqlStepMicros(0, HOUR, 15_000_000)).toBe(15_000_000);
  });

  it('is never finer than a second', () => {
    expect(promqlStepMicros(0, 60_000_000)).toBe(1_000_000);
    expect(promqlStepMicros(0, HOUR, 10)).toBe(1_000_000);
  });

  it('coarsens a chosen step the server would refuse', () => {
    const step = promqlStepMicros(0, 7 * DAY, 1_000_000);

    expect(7 * DAY / step + 1).toBeLessThanOrEqual(PROMQL_MAX_STEPS);
  });
});
