import dayjs from 'dayjs';
import { describe, expect, it, vi } from 'vitest';

import { formatMicros, tzOffsetLabel, zonedMoment } from './time';

const SAMPLE_MICROS = Date.UTC(2026, 6, 27, 16, 5, 9) * 1000;

describe('formatMicros date and time preferences', () => {
  it('formats each supported date order independently from the time system', () => {
    expect(
      formatMicros(
        SAMPLE_MICROS,
        'UTC',
        'iso_24h',
        true,
        'yyyy_mm_dd_dash',
      ),
    ).toBe('2026-07-27 16:05:09');
    expect(
      formatMicros(
        SAMPLE_MICROS,
        'UTC',
        'iso_24h',
        true,
        'yyyy_mm_dd_slash',
      ),
    ).toBe('2026/07/27 16:05:09');
    expect(
      formatMicros(
        SAMPLE_MICROS,
        'UTC',
        'iso_24h',
        true,
        'dd_mm_yyyy_slash',
      ),
    ).toBe('27/07/2026 16:05:09');
    expect(
      formatMicros(
        SAMPLE_MICROS,
        'UTC',
        'iso_24h',
        true,
        'mm_dd_yyyy_slash',
      ),
    ).toBe('07/27/2026 16:05:09');
  });

  it('uses the selected 12-hour clock without changing the date order', () => {
    expect(
      formatMicros(
        SAMPLE_MICROS,
        'UTC',
        'local_12h',
        false,
        'dd_mm_yyyy_slash',
      ),
    ).toBe('27/07/2026 04:05 PM');
  });

  it('normalizes a zero UTC offset', () => {
    expect(tzOffsetLabel('UTC')).toBe('UTC');
  });
});

describe('zonedMoment', () => {
  const PATTERN = 'YYYY-MM-DD hh:mm:ss.SSS A HH';
  /** Every five minutes for three hours on either side of a moment. */
  const around = (iso: string) =>
    Array.from({ length: 73 }, (_, step) => Date.parse(iso) + (step - 36) * 5 * 60_000 + 123);

  it.each([
    // Clocks go forward and back at 02:00 in New York.
    ['America/New_York', '2026-03-08T07:00:00Z'],
    ['America/New_York', '2026-11-01T06:00:00Z'],
    // Lord Howe moves by half an hour, on a half hour of UTC.
    ['Australia/Lord_Howe', '2026-10-03T15:30:00Z'],
    // Offsets that are not whole hours, and a zone without a change.
    ['Asia/Kathmandu', '2026-05-23T18:15:00Z'],
    ['Asia/Shanghai', '2026-05-23T16:00:00Z'],
    ['UTC', '2026-12-31T23:59:59Z'],
  ])('shows the wall clock of %s around %s as the zone database does', (tz, iso) => {
    for (const ms of around(iso)) {
      expect(zonedMoment(ms, tz).format(PATTERN)).toBe(dayjs(ms).tz(tz).format(PATTERN));
    }
  });

  it('asks the zone once per quarter hour, not once per moment', () => {
    const asked = vi.spyOn(Object.getPrototypeOf(dayjs()) as { tz: (zone: string) => unknown }, 'tz');
    // A zone and a year no other test has asked about.
    const start = Date.parse('2031-05-23T10:00:00Z');

    // Five thousand moments 1.37 s apart, back from 10:00:00 to 08:05:51: the quarter hour
    // that begins at 10:00 and the eight before it.
    for (let index = 0; index < 5_000; index += 1) {
      zonedMoment(start - index * 1_370, 'Asia/Tokyo').format('HH:mm:ss.SSS');
    }

    expect(asked).toHaveBeenCalledTimes(9);
    asked.mockRestore();
  });

  it('has no clock to show for what is not a moment', () => {
    expect(zonedMoment(Number.NaN, 'UTC').isValid()).toBe(false);
    expect(formatMicros(Number.NaN, 'UTC', 'iso_24h')).toBe('—');
  });
});

