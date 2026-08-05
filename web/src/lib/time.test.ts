import { describe, expect, it } from 'vitest';

import { formatMicros, tzOffsetLabel } from './time';

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
