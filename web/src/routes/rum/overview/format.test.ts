import { describe, expect, it } from 'vitest';

import { formatCount, formatPercent } from './format';

describe('the figures of the overview', () => {
  it('count in full below ten thousand and compactly from there', () => {
    expect(formatCount(4_812, 'en-US')).toBe('4,812');
    expect(formatCount(123_456, 'en-US')).toBe('123.5K');
    expect(formatCount(123_456, 'zh-CN')).toBe('12.3万');
  });

  it('are shares in whole percent, small ones not rounded away to nothing', () => {
    expect(formatPercent(0.384)).toBe('38%');
    expect(formatPercent(0.0049)).toBe('<1%');
    expect(formatPercent(1)).toBe('100%');
    expect(formatPercent(0)).toBe('0%');
    expect(formatPercent(Number.NaN)).toBe('0%');
  });
});
