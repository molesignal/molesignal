import { describe, expect, it } from 'vitest';

import { daysLabel, newRange, normalizeClock, rangeLabel, rangeProblem, weekdayLabel } from './model';

describe('weekly ranges', () => {
  it('names the days from Sunday, as the mask counts them', () => {
    expect(weekdayLabel(0, 'en-US')).toBe('Sun');
    expect(weekdayLabel(6, 'en-US')).toBe('Sat');
    expect(daysLabel(0b0111110, 'en-US')).toBe('Mon–Fri');
    expect(daysLabel(0b0101010, 'en-US')).toBe('Mon, Wed, Fri');
    expect(daysLabel(0, 'en-US')).toBe('—');
  });

  it('pads a typed time and leaves the rest for the checks', () => {
    expect(normalizeClock(' 9:30 ')).toBe('09:30');
    expect(normalizeClock('9：05')).toBe('09:05');
    expect(normalizeClock('later')).toBe('later');
  });

  it('points out what the server would refuse', () => {
    expect(rangeProblem(newRange())).toBeNull();
    expect(rangeProblem({ weekdayMask: 0, start: '09:00', end: '18:00' })).toBe('no_days');
    expect(rangeProblem({ weekdayMask: 2, start: '09:00', end: '09:00' })).toBe('same_time');
    expect(rangeProblem({ weekdayMask: 2, start: '24:00', end: '06:00' })).toBe('bad_time');
    expect(rangeProblem({ weekdayMask: 2, start: '22:00', end: '24:00' })).toBeNull();
    expect(rangeProblem({ weekdayMask: 2, start: '9:00', end: '18:00' })).toBe('bad_time');
  });

  it('writes a range as its days and times', () => {
    expect(rangeLabel({ weekdayMask: 0b0111110, start: '09:30', end: '18:00' }, 'en-US')).toBe(
      'Mon–Fri 09:30–18:00',
    );
  });
});
