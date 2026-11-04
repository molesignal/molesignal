import { describe, expect, it } from 'vitest';

import { formatClock } from './clock';

describe('a moment of a session', () => {
  it('is read in minutes and seconds from the beginning', () => {
    expect(formatClock(0)).toBe('0:00');
    expect(formatClock(38_000)).toBe('0:38');
    expect(formatClock(62_999)).toBe('1:02');
    expect(formatClock(228_000)).toBe('3:48');
  });

  it('keeps the tenths where the rows of a list need them', () => {
    expect(formatClock(41_500, true)).toBe('0:41.5');
    expect(formatClock(41_549, true)).toBe('0:41.5');
    expect(formatClock(59_999, true)).toBe('0:59.9');
    expect(formatClock(60_000, true)).toBe('1:00.0');
  });

  it('puts the hours first past the hour', () => {
    expect(formatClock(3_723_000)).toBe('1:02:03');
  });

  it('is the beginning for what is before it or is not a number', () => {
    expect(formatClock(-5_000)).toBe('0:00');
    expect(formatClock(Number.NaN)).toBe('0:00');
  });
});
