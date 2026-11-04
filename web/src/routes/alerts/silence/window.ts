import type { TFunction } from 'i18next';

import { formatMicrosActive } from '@/lib/time';
import { rangesLabel, weekdayLabel } from '@/shell/weekly-ranges/model';
import type { MonthDay, MuteRule, MuteWindow } from '@/types/alerting';

export const DEFAULT_SILENCE_TIMEZONE = 'UTC';

/** The longest a monthly window lasts, in minutes: seven days, as the server allows. */
export const MAX_MONTHLY_MINUTES = 7 * 24 * 60;

/** The ordinals a monthly window can name: the first to the fifth, and the last. */
export const MONTH_ORDINALS = [1, 2, 3, 4, 5, -1] as const;

/** A new silence starts as the hour that begins now. */
export function defaultSilenceWindow(nowMillis = Date.now()): MuteWindow {
  return { type: 'fixed', start: nowMillis * 1000, end: (nowMillis + 60 * 60 * 1000) * 1000 };
}

/** What the window select shows: a window kind, or holidays (every day, whole days, holidays only). */
export type SilenceKind = MuteWindow['type'] | 'holidays';

const WHOLE_DAYS = { weekdayMask: 0b111_1111, start: '00:00', end: '24:00' };

/** A weekly window over whole days that counts holidays only reads as the holidays of a calendar. */
export function kindOfWindow(window: MuteWindow): SilenceKind {
  const wholeDays =
    window.type === 'weekly'
    && window.ranges.length === 1
    && window.ranges[0]?.weekdayMask === WHOLE_DAYS.weekdayMask
    && window.ranges[0].start === WHOLE_DAYS.start
    && window.ranges[0].end === WHOLE_DAYS.end;

  return window.type === 'weekly' && wholeDays && window.calendar?.holidays === 'only' ? 'holidays' : window.type;
}

/** The window a silence gets when its kind is switched, in the zone it already had. */
export function windowOfKind(
  kind: SilenceKind,
  timezone = DEFAULT_SILENCE_TIMEZONE,
  calendarId = '',
): MuteWindow {
  if (kind === 'holidays') {
    return { type: 'weekly', timezone, ranges: [WHOLE_DAYS], calendar: { calendarId: calendarId, holidays: 'only' } };
  }
  if (kind === 'weekly') {
    return { type: 'weekly', timezone, ranges: [WHOLE_DAYS] };
  }
  if (kind === 'monthly') {
    return {
      type: 'monthly',
      timezone,
      day: { kind: 'weekday', ordinal: 1, weekday: 0 },
      start: '02:00',
      durationMinutes: 240,
    };
  }
  return defaultSilenceWindow();
}

/** "4 h", "90 min", "1 h 30 min" in the reader's language. */
export function durationLabel(minutes: number, t: TFunction<'alerts'>): string {
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) return t('silences.duration.minutes', { count: rest });
  if (rest === 0) return t('silences.duration.hours', { count: hours });
  return t('silences.duration.hours_minutes', { hours, minutes: rest });
}

function monthDayLabel(day: MonthDay, t: TFunction<'alerts'>, locale?: string): string {
  if (day.kind === 'date') return t('silences.month_day.date', { day: day.day });
  return t('silences.month_day.weekday', {
    ordinal: t(`silences.ordinals.${day.ordinal === -1 ? 'last' : String(day.ordinal)}`),
    weekday: weekdayLabel(day.weekday, locale),
  });
}

/** When a silence applies, in the time format the rest of the app uses. */
export function windowSummary(window: MuteWindow, t: TFunction<'alerts'>, locale?: string): string {
  if (window.type === 'fixed') {
    return `${formatMicrosActive(window.start, false)} → ${formatMicrosActive(window.end, false)}`;
  }
  if (kindOfWindow(window) === 'holidays') {
    return t('silences.summary.holidays', { timezone: window.type === 'weekly' ? window.timezone : '' });
  }
  if (window.type === 'weekly') {
    const holidays = window.calendar ? ` · ${t(`silences.summary.calendar_${window.calendar.holidays}`)}` : '';
    return `${rangesLabel(window.ranges, locale)} ${window.timezone}${holidays}`;
  }
  return t('silences.summary.monthly', {
    day: monthDayLabel(window.day, t, locale),
    start: window.start,
    duration: durationLabel(window.durationMinutes, t),
    timezone: window.timezone,
  });
}

/**
 * How a silence stands at a moment:
 * - `off`: switched off, whatever its window;
 * - `active`: it is pausing delivery now;
 * - `upcoming`: a fixed window that has not begun;
 * - `ended`: a fixed window that is over;
 * - `waiting`: a repeating window between two of its times.
 *
 * A repeating window is placed by the server (`active`), in its zone and to the minute.
 */
export type SilenceStanding = 'off' | 'active' | 'upcoming' | 'ended' | 'waiting';

export function silenceStanding(
  rule: Pick<MuteRule, 'enabled' | 'window' | 'active'>,
  nowMicros: number,
): SilenceStanding {
  if (!rule.enabled) return 'off';

  const { window } = rule;

  if (window.type === 'fixed') {
    if (nowMicros < window.start) return 'upcoming';

    return nowMicros < window.end ? 'active' : 'ended';
  }

  return rule.active ? 'active' : 'waiting';
}
