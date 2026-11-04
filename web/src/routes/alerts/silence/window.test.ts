import type { TFunction } from 'i18next';
import { beforeAll, describe, expect, it } from 'vitest';

import { setActiveTimePreference } from '@/lib/time';
import type { MuteWindow } from '@/types/alerting';

import {
  defaultSilenceWindow,
  durationLabel,
  silenceStanding,
  windowOfKind,
  windowSummary,
} from './window';

const micros = (iso: string) => Date.parse(iso) * 1000;

/** Writes the key and its values, so a summary can be checked without the translations. */
const t = ((key: string, values?: Record<string, unknown>) =>
  values ? `${key}${JSON.stringify(values)}` : key) as unknown as TFunction<'alerts'>;

describe('silence windows', () => {
  beforeAll(() => {
    setActiveTimePreference('UTC', 'iso_24h', 'yyyy_mm_dd_dash');
  });

  it('starts a new silence as the hour that begins now', () => {
    const window = defaultSilenceWindow(Date.parse('2026-05-23T10:00:00Z'));

    expect(window).toEqual({
      type: 'fixed',
      start: micros('2026-05-23T10:00:00Z'),
      end: micros('2026-05-23T11:00:00Z'),
    });
  });

  it('switches kinds keeping the zone it had', () => {
    expect(windowOfKind('weekly', 'Asia/Shanghai')).toEqual({
      type: 'weekly',
      timezone: 'Asia/Shanghai',
      ranges: [{ weekdayMask: 0b111_1111, start: '00:00', end: '24:00' }],
    });
    expect(windowOfKind('monthly')).toMatchObject({ type: 'monthly', timezone: 'UTC', durationMinutes: 240 });
  });

  it('writes a fixed window in the app time format, not the browser locale', () => {
    expect(
      windowSummary(
        {
          type: 'fixed',
          start: micros('2026-05-23T09:40:00Z'),
          end: micros('2026-05-23T11:40:00Z'),
        },
        t,
      ),
    ).toBe('2026-05-23 09:40 → 2026-05-23 11:40');
  });

  it('writes a weekly window as its ranges and its zone', () => {
    const window: MuteWindow = {
      type: 'weekly',
      timezone: 'UTC',
      ranges: [
        { weekdayMask: 0b0111110, start: '20:00', end: '06:00' },
        { weekdayMask: 0b1000001, start: '00:00', end: '24:00' },
      ],
    };

    expect(windowSummary(window, t, 'en-US')).toBe('Mon–Fri 20:00–06:00; Sun, Sat 00:00–24:00 UTC');
  });

  it('writes a monthly window from its day, start and length', () => {
    const window: MuteWindow = {
      type: 'monthly',
      timezone: 'Asia/Shanghai',
      day: { kind: 'weekday', ordinal: -1, weekday: 0 },
      start: '02:00',
      durationMinutes: 90,
    };
    const summary = windowSummary(window, t, 'en-US');

    expect(summary).toContain('silences.summary.monthly');
    expect(summary).toContain('silences.ordinals.last');
    expect(summary).toContain('"start":"02:00"');
    expect(durationLabel(90, t)).toBe('silences.duration.hours_minutes{"hours":1,"minutes":30}');
    expect(durationLabel(120, t)).toBe('silences.duration.hours{"count":2}');
    expect(durationLabel(45, t)).toBe('silences.duration.minutes{"count":45}');
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

  it('takes the server’s word on whether a repeating window is on now', () => {
    const window = windowOfKind('weekly');

    expect(silenceStanding({ enabled: true, window, active: true }, 0)).toBe('active');
    expect(silenceStanding({ enabled: true, window, active: false }, 0)).toBe('waiting');
    expect(silenceStanding({ enabled: true, window }, 0)).toBe('waiting');
    expect(silenceStanding({ enabled: false, window, active: true }, 0)).toBe('off');
  });
});
