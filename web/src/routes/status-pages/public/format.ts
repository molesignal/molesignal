import type { TFunction } from 'i18next';

import { formatRelativeSpan } from '@/time/relative';

import { formatDurationMicros } from '../model';

/** A moment in microseconds, as the `datetime` of a `<time>`. */
export function microsDateTime(micros: number): string {
  return new Date(Math.floor(micros / 1_000)).toISOString();
}

/**
 * How far a moment is from another, in words and in the page's language:
 * "38 minutes ago", "in 2 days". The distance is measured from when the page
 * was generated, not from the reader's clock, so it says the same to everyone
 * looking at the same page.
 */
export function relativeTime(target: number, from: number, language: string): string {
  return formatRelativeSpan(target, from, language);
}

const DAY_MICROS = 24 * 60 * 60 * 1_000_000;

/**
 * How long something lasted. Minutes and hours while that is how one would say
 * it; whole days once it ran for two or more, as planned work often does.
 */
export function lasted(copy: TFunction, startedAt: number, endedAt: number): string {
  const days = Math.floor(Math.max(0, endedAt - startedAt) / DAY_MICROS);

  return days >= 2 ? copy('values.days', { count: days }) : formatDurationMicros(copy, startedAt, endedAt);
}

/**
 * Several names as one would write them in a line: "API and Dashboard", "API,
 * Dashboard, and Billing". Chinese sets a list with the enumeration comma, which
 * also keeps names written in Latin letters from running into the word between.
 */
export function listNames(names: string[], language: string): string {
  if (language.toLowerCase().startsWith('zh')) return names.join('、');

  return new Intl.ListFormat(language, { style: 'long', type: 'conjunction' }).format(names);
}

/** The day of the month a moment falls on where the page is, as a bare number. */
export function zonedDayOfMonth(micros: number, timezone: string): string {
  const parts = new Intl.DateTimeFormat('en-US', { day: 'numeric', timeZone: timezone }).formatToParts(
    new Date(Math.floor(micros / 1_000)),
  );

  return parts.find((part) => part.type === 'day')?.value ?? '';
}

/** A day in the page's time zone, as a key that sorts and as the heading it is shown under. */
export function zonedDay(micros: number, timezone: string, language: string): { key: string; label: string } {
  const date = new Date(Math.floor(micros / 1_000));
  const parts = Object.fromEntries(
    new Intl.DateTimeFormat('en-US', { year: 'numeric', month: '2-digit', day: '2-digit', timeZone: timezone })
      .formatToParts(date)
      .map(({ type, value }) => [type, value]),
  );

  return {
    key: `${parts.year}-${parts.month}-${parts.day}`,
    label: new Intl.DateTimeFormat(language, { year: 'numeric', month: 'long', day: 'numeric', timeZone: timezone }).format(date),
  };
}
