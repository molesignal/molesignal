import { describe, expect, it } from 'vitest';

import { greetingPeriodForHour } from './greeting';

describe('Mole Intelligence time-based greeting', () => {
  it.each([
    [0, 'late'],
    [4, 'late'],
    [5, 'morning'],
    [11, 'morning'],
    [12, 'afternoon'],
    [17, 'afternoon'],
    [18, 'evening'],
    [22, 'evening'],
    [23, 'late'],
  ] as const)('maps local hour %i to %s', (hour, period) => {
    expect(greetingPeriodForHour(hour)).toBe(period);
  });
});
