import { describe, expect, it } from 'vitest';

import {
  formatInterval,
  intervalFromSeconds,
  intervalSeconds,
  parseInterval,
} from './schedule';

describe('pipeline schedule', () => {
  it('reads the interval the backend stores', () => {
    expect(parseInterval('every:5m')).toEqual({ count: 5, unit: 'm' });
    expect(parseInterval(' every:90s ')).toEqual({ count: 90, unit: 's' });
    expect(parseInterval('every:1h')).toEqual({ count: 1, unit: 'h' });
  });

  it('falls back to five minutes for what the backend would not run', () => {
    for (const cron of [undefined, null, '', '0 0 * * *', 'every:0m', 'every:5d', 'every:m']) {
      expect(parseInterval(cron)).toEqual({ count: 5, unit: 'm' });
    }
  });

  it('writes the interval back in the same form', () => {
    expect(formatInterval({ count: 15, unit: 's' })).toBe('every:15s');
    expect(formatInterval(parseInterval('every:2h'))).toBe('every:2h');
    // Never a schedule the backend rejects.
    expect(formatInterval({ count: 0, unit: 'm' })).toBe('every:1m');
    expect(formatInterval({ count: 2.9, unit: 'h' })).toBe('every:2h');
    expect(formatInterval({ count: Number.NaN, unit: 'h' })).toBe('every:1h');
  });

  it('shows a lookback in the largest unit that holds it exactly', () => {
    expect(intervalFromSeconds(300)).toEqual({ count: 5, unit: 'm' });
    expect(intervalFromSeconds(7200)).toEqual({ count: 2, unit: 'h' });
    expect(intervalFromSeconds(90)).toEqual({ count: 90, unit: 's' });
    expect(intervalFromSeconds(0)).toEqual({ count: 5, unit: 'm' });
    expect(intervalFromSeconds(undefined)).toEqual({ count: 5, unit: 'm' });
  });

  it('turns a count and a unit into seconds', () => {
    expect(intervalSeconds({ count: 5, unit: 'm' })).toBe(300);
    expect(intervalSeconds({ count: 2, unit: 'h' })).toBe(7200);
    expect(intervalSeconds(intervalFromSeconds(90))).toBe(90);
    expect(intervalSeconds({ count: 0, unit: 's' })).toBe(1);
  });
});
