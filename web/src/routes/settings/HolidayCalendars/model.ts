import type { HolidayDay } from '@/api/holidays';

/** `YYYY-MM-DD` of a local date. */
export function isoDate(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${date.getFullYear()}-${month}-${day}`;
}

/** The days sorted by date. */
export function sortedDays(days: readonly HolidayDay[]): HolidayDay[] {
  return [...days].sort((left, right) => left.date.localeCompare(right.date));
}

/** Days read from a file go over the days already there: a date the file names is replaced. */
export function mergeDays(current: readonly HolidayDay[], incoming: readonly HolidayDay[]): HolidayDay[] {
  const byDate = new Map(current.map((day) => [day.date, day]));
  for (const day of incoming) byDate.set(day.date, day);
  return sortedDays([...byDate.values()]);
}

/** The years a calendar has days in, and the current one, newest first. */
export function yearsOf(days: readonly HolidayDay[], today: string): string[] {
  const years = new Set(days.map((day) => day.date.slice(0, 4)));
  years.add(today.slice(0, 4));
  return [...years].sort().reverse();
}

export function daysOfYear(days: readonly HolidayDay[], year: string): HolidayDay[] {
  return sortedDays(days.filter((day) => day.date.startsWith(`${year}-`)));
}

/** How many days off and make-up workdays a calendar has. */
export function dayCounts(days: readonly HolidayDay[]): { holidays: number; workdays: number } {
  const workdays = days.filter((day) => day.kind === 'workday').length;
  return { holidays: days.length - workdays, workdays };
}

/** The first day off on or after today, if any. */
export function nextHoliday(days: readonly HolidayDay[], today: string): HolidayDay | null {
  return sortedDays(days).find((day) => day.kind === 'holiday' && day.date >= today) ?? null;
}
