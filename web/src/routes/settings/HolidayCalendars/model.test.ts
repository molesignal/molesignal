import { describe, expect, it } from 'vitest';

import type { HolidayDay } from '@/api/holidays';

import { dayCounts, daysOfYear, mergeDays, nextHoliday, yearsOf } from './model';

const day = (date: string, kind: HolidayDay['kind'] = 'holiday', name = ''): HolidayDay => ({ date, kind, name });

describe('holiday calendar model', () => {
  it('lets the days of a file replace the same dates and keeps the rest', () => {
    const merged = mergeDays(
      [day('2026-10-01', 'holiday', 'old'), day('2026-05-01')],
      [day('2026-10-01', 'holiday', 'National Day'), day('2026-10-10', 'workday')],
    );

    expect(merged.map((item) => [item.date, item.kind, item.name])).toEqual([
      ['2026-05-01', 'holiday', ''],
      ['2026-10-01', 'holiday', 'National Day'],
      ['2026-10-10', 'workday', ''],
    ]);
  });

  it('lists the years with days, the current one too, and the days of one year', () => {
    const days = [day('2025-12-25'), day('2027-01-01'), day('2026-10-10', 'workday')];

    expect(yearsOf(days, '2026-10-29')).toEqual(['2027', '2026', '2025']);
    expect(daysOfYear(days, '2026').map((item) => item.date)).toEqual(['2026-10-10']);
    expect(dayCounts(days)).toEqual({ holidays: 2, workdays: 1 });
  });

  it('finds the next day off, never a make-up workday', () => {
    const days = [day('2026-10-01'), day('2026-10-31', 'workday'), day('2027-01-01')];

    expect(nextHoliday(days, '2026-10-29')?.date).toBe('2027-01-01');
    expect(nextHoliday(days, '2027-01-02')).toBeNull();
  });
});
