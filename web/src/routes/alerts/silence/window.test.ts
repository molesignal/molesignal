import { beforeAll, describe, expect, it } from 'vitest';

import { setActiveTimePreference } from '@/lib/time';
import type { MuteWindow } from '@/types/alerting';

import {
  clampHour,
  defaultSilenceWindow,
  silenceStanding,
  weekdayLabel,
  windowSummary,
} from './window';

const micros = (iso: string) => Date.parse(iso) * 1000;

describe('silence windows', () => {
  beforeAll(() => {
    setActiveTimePreference('UTC', 'iso_24h', 'yyyy_mm_dd_dash');
  });

  it('names the days from Sunday, as the mask counts them', () => {
    expect(weekdayLabel(0, 'en-US')).toBe('Sun');
    expect(weekdayLabel(6, 'en-US')).toBe('Sat');
  });

  it('starts a new silence as the hour that begins now', () => {
    const window = defaultSilenceWindow(Date.parse('2026-05-23T10:00:00Z'));

    expect(window).toEqual({
      type: 'fixed',
      start: micros('2026-05-23T10:00:00Z'),
      end: micros('2026-05-23T11:00:00Z'),
    });
  });

  it('keeps an hour inside the day', () => {
    expect(clampHour('7.9')).toBe(7);
    expect(clampHour('-3')).toBe(0);
    expect(clampHour('99')).toBe(24);
    expect(clampHour('night')).toBe(0);
  });

  it('writes a fixed window in the app time format, not the browser locale', () => {
    expect(
      windowSummary({
        type: 'fixed',
        start: micros('2026-05-23T09:40:00Z'),
        end: micros('2026-05-23T11:40:00Z'),
      }),
    ).toBe('2026-05-23 09:40 → 2026-05-23 11:40');
  });

  it('writes a recurring window as its days, its hours and its zone', () => {
    const window = (weekday_mask: number): MuteWindow => ({
      type: 'recurring',
      timezone: 'UTC',
      weekday_mask,
      hour_start: 20,
      hour_end: 6,
    });

    // Monday to Friday is a run, so it is written as one.
    expect(windowSummary(window(0b0111110), 'en-US')).toBe('Mon–Fri · 20:00–06:00 UTC');
    expect(windowSummary(window(0b0101010), 'en-US')).toBe('Mon, Wed, Fri · 20:00–06:00 UTC');
    expect(windowSummary(window(0b1000001), 'en-US')).toBe('Sun, Sat · 20:00–06:00 UTC');
    expect(windowSummary(window(0), 'en-US')).toBe('— · 20:00–06:00 UTC');
  });

  it('says how a fixed silence stands at a moment', () => {
    const window: MuteWindow = {
      type: 'fixed',
      start: micros('2026-05-23T09:00:00Z'),
      end: micros('2026-05-23T11:00:00Z'),
    };

    expect(silenceStanding({ enabled: true, window }, micros('2026-05-23T08:59:00Z'))).toBe('upcoming');
    expect(silenceStanding({ enabled: true, window }, micros('2026-05-23T09:00:00Z'))).toBe('active');
    expect(silenceStanding({ enabled: true, window }, micros('2026-05-23T11:00:00Z'))).toBe('ended');
    expect(silenceStanding({ enabled: false, window }, micros('2026-05-23T10:00:00Z'))).toBe('off');
  });

  it('places a recurring window in its own time zone, over midnight too', () => {
    // 23 May 2026 is a Saturday.
    const nightly: MuteWindow = {
      type: 'recurring',
      timezone: 'UTC',
      weekday_mask: 0b1000000,
      hour_start: 22,
      hour_end: 6,
    };

    expect(silenceStanding({ enabled: true, window: nightly }, micros('2026-05-23T23:30:00Z'))).toBe('active');
    expect(silenceStanding({ enabled: true, window: nightly }, micros('2026-05-23T03:00:00Z'))).toBe('active');
    expect(silenceStanding({ enabled: true, window: nightly }, micros('2026-05-23T12:00:00Z'))).toBe('waiting');
    // The same moment is already Sunday in Shanghai, which the mask leaves out.
    expect(
      silenceStanding(
        { enabled: true, window: { ...nightly, timezone: 'Asia/Shanghai' } },
        micros('2026-05-23T23:30:00Z'),
      ),
    ).toBe('waiting');
    // A zone nobody knows cannot be placed, so it is not claimed to be silencing.
    expect(
      silenceStanding(
        { enabled: true, window: { ...nightly, timezone: 'Mars/Olympus' } },
        micros('2026-05-23T23:30:00Z'),
      ),
    ).toBe('waiting');
  });
});
