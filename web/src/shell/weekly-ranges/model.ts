import type { WeeklyRange } from '@/types/alerting';

/** The days of a range, Sunday first: bit 0 of the mask is Sunday, as the server reads it. */
export const WEEKDAYS = [0, 1, 2, 3, 4, 5, 6] as const;

export const WORKDAYS_MASK = 0b011_1110;
export const EVERY_DAY_MASK = 0b111_1111;

/** A new range: working days, 09:00 to 18:00. */
export function newRange(): WeeklyRange {
  return { weekdayMask: WORKDAYS_MASK, start: '09:00', end: '18:00' };
}

/** The short name of a weekday, by its bit in a range's mask. */
export function weekdayLabel(day: number, locale?: string): string {
  // 1 January 2023 was a Sunday.
  return new Intl.DateTimeFormat(locale, { weekday: 'short', timeZone: 'UTC' }).format(
    new Date(Date.UTC(2023, 0, 1 + day)),
  );
}

/** "Mon–Fri" for a run of days, "Mon, Wed, Fri" otherwise, and a dash for none at all. */
export function daysLabel(mask: number, locale?: string): string {
  const days = WEEKDAYS.filter((day) => (mask & (1 << day)) !== 0);
  if (days.length === 0) return '—';

  const first = days[0] ?? 0;
  const last = days[days.length - 1] ?? first;
  const consecutive = days.length > 2 && last - first === days.length - 1;

  return consecutive
    ? `${weekdayLabel(first, locale)}–${weekdayLabel(last, locale)}`
    : days.map((day) => weekdayLabel(day, locale)).join(', ');
}

/** "9:30" → "09:30"; anything else is left for `rangeProblem` to point out. */
export function normalizeClock(text: string): string {
  const match = /^\s*(\d{1,2})[:：](\d{2})\s*$/.exec(text);
  return match ? `${match[1]!.padStart(2, '0')}:${match[2]}` : text.trim();
}

/** "Mon–Fri 09:30–18:00"; an end not after the start runs into the next day. */
export function rangeLabel(range: WeeklyRange, locale?: string): string {
  return `${daysLabel(range.weekdayMask, locale)} ${range.start}–${range.end}`;
}

export function rangesLabel(ranges: readonly WeeklyRange[], locale?: string): string {
  return ranges.map((range) => rangeLabel(range, locale)).join('; ');
}

/** What keeps the server from taking a range, if anything. */
export type RangeProblem = 'no_days' | 'same_time' | 'bad_time';

const CLOCK = /^([01]\d|2[0-3]):[0-5]\d$/;

export function rangeProblem(range: WeeklyRange): RangeProblem | null {
  if ((range.weekdayMask & EVERY_DAY_MASK) === 0) return 'no_days';
  if (!CLOCK.test(range.start) || (range.end !== '24:00' && !CLOCK.test(range.end))) {
    return 'bad_time';
  }
  return range.start === range.end ? 'same_time' : null;
}
