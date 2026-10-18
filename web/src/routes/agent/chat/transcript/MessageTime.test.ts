import { describe, expect, it } from 'vitest';

import { daysAgo } from './MessageTime';

describe('message time', () => {
  it('counts calendar days, not 24-hour spans', () => {
    expect(daysAgo(new Date(2026, 9, 17, 0, 5), new Date(2026, 9, 17, 23, 0))).toBe(0);
    expect(daysAgo(new Date(2026, 9, 16, 23, 59), new Date(2026, 9, 17, 0, 1))).toBe(1);
    expect(daysAgo(new Date(2026, 9, 14, 8, 0), new Date(2026, 9, 17, 7, 0))).toBe(3);
  });
});
