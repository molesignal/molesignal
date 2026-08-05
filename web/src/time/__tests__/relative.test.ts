import { describe, expect, it } from 'vitest';

import { formatRelativeMicros } from '../relative';

const NOW_MICROS = 1_700_000_000_000_000;

describe('relative time presentation', () => {
  it('formats timestamps in the selected interface language', () => {
    expect(
      formatRelativeMicros(
        NOW_MICROS - 30 * 1_000_000,
        'en-us',
        NOW_MICROS,
      ),
    ).toBe('now');
    expect(
      formatRelativeMicros(
        NOW_MICROS - 2 * 60 * 1_000_000,
        'en-us',
        NOW_MICROS,
      ),
    ).toBe('2 minutes ago');
    expect(
      formatRelativeMicros(
        NOW_MICROS - 2 * 60 * 1_000_000,
        'zh-cn',
        NOW_MICROS,
      ),
    ).toBe('2分钟前');
  });

  it('supports epoch seconds and missing timestamps', () => {
    expect(
      formatRelativeMicros(
        (NOW_MICROS - 2 * 60 * 1_000_000) / 1_000_000,
        'en-us',
        NOW_MICROS,
      ),
    ).toBe('2 minutes ago');
    expect(formatRelativeMicros(null, 'en-us', NOW_MICROS)).toBe('—');
  });
});
