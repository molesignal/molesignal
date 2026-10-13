import { formatMicrosActive } from '@/lib/time';
import type { MuteRule, MuteWindow } from '@/types/alerting';

export const DEFAULT_SILENCE_TIMEZONE = 'UTC';

/** The days of a recurring window, Sunday first: bit 0 of the mask is Sunday, as the server reads it. */
const WEEKDAYS = [0, 1, 2, 3, 4, 5, 6] as const;

/** The short name of a weekday, by its bit in a window's mask. */
export function weekdayLabel(day: number, locale?: string): string {
  // 1 January 2023 was a Sunday.
  return new Intl.DateTimeFormat(locale, { weekday: 'short', timeZone: 'UTC' }).format(
    new Date(Date.UTC(2023, 0, 1 + day)),
  );
}

/** A new silence starts as the hour that begins now. */
export function defaultSilenceWindow(nowMillis = Date.now()): MuteWindow {
  return { type: 'fixed', start: nowMillis * 1000, end: (nowMillis + 60 * 60 * 1000) * 1000 };
}

export function clampHour(value: string): number {
  const hour = Math.floor(Number(value));

  if (!Number.isFinite(hour)) return 0;

  return Math.min(24, Math.max(0, hour));
}

/** When a silence applies, in the time format the rest of the app uses. */
export function windowSummary(window: MuteWindow, locale?: string): string {
  if (window.type === 'fixed') {
    return `${formatMicrosActive(window.start, false)} → ${formatMicrosActive(window.end, false)}`;
  }

  const days = WEEKDAYS.filter((day) => (window.weekday_mask & (1 << day)) !== 0);
  const hours = `${pad(window.hour_start)}:00–${pad(window.hour_end)}:00`;

  return `${daysLabel(days, locale)} · ${hours} ${window.timezone}`;
}

function pad(hour: number): string {
  return String(hour).padStart(2, '0');
}

/** "Mon–Fri" for a run of days, "Mon, Wed, Fri" otherwise, and a word for none at all. */
function daysLabel(days: readonly number[], locale?: string): string {
  if (days.length === 0) return '—';

  const first = days[0] ?? 0;
  const last = days[days.length - 1] ?? first;
  const consecutive = days.length > 2 && last - first === days.length - 1;

  return consecutive
    ? `${weekdayLabel(first, locale)}–${weekdayLabel(last, locale)}`
    : days.map((day) => weekdayLabel(day, locale)).join(', ');
}

/**
 * How a silence stands at a moment:
 * - `off`: switched off, whatever its window;
 * - `active`: it is pausing delivery now;
 * - `upcoming`: a fixed window that has not begun;
 * - `ended`: a fixed window that is over;
 * - `waiting`: a recurring window between two of its hours.
 */
export type SilenceStanding = 'off' | 'active' | 'upcoming' | 'ended' | 'waiting';

export function silenceStanding(rule: Pick<MuteRule, 'enabled' | 'window'>, nowMicros: number): SilenceStanding {
  if (!rule.enabled) return 'off';

  const { window } = rule;

  if (window.type === 'fixed') {
    if (nowMicros < window.start) return 'upcoming';

    return nowMicros < window.end ? 'active' : 'ended';
  }

  return recurringWindowActive(window, nowMicros) ? 'active' : 'waiting';
}

/** The server's own rule: the local day must be in the mask, and the local hour inside the hours. */
function recurringWindowActive(window: Extract<MuteWindow, { type: 'recurring' }>, nowMicros: number): boolean {
  const local = localDayAndHour(nowMicros, window.timezone);

  if (!local || (window.weekday_mask & (1 << local.day)) === 0) return false;

  return window.hour_start <= window.hour_end
    ? window.hour_start <= local.hour && local.hour < window.hour_end
    // A window over midnight, such as 22 to 6.
    : local.hour >= window.hour_start || local.hour < window.hour_end;
}

const DAY_INDEX: Record<string, number> = { Sun: 0, Mon: 1, Tue: 2, Wed: 3, Thu: 4, Fri: 5, Sat: 6 };

function localDayAndHour(nowMicros: number, timezone: string): { day: number; hour: number } | null {
  try {
    const parts = new Intl.DateTimeFormat('en-US', {
      weekday: 'short',
      hour: 'numeric',
      hourCycle: 'h23',
      timeZone: timezone || DEFAULT_SILENCE_TIMEZONE,
    }).formatToParts(new Date(Math.floor(nowMicros / 1000)));
    const day = DAY_INDEX[parts.find((part) => part.type === 'weekday')?.value ?? ''];
    const hour = Number(parts.find((part) => part.type === 'hour')?.value);

    return day === undefined || !Number.isFinite(hour) ? null : { day, hour };
  } catch {
    // A zone the browser does not know: the window cannot be placed, so it is not claimed to be active.
    return null;
  }
}
