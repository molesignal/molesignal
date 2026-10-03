import { describe, expect, it } from 'vitest';

import { formatBytesCompact, splitMetricValue } from './format';

describe('splitMetricValue', () => {
  it('separates the figure from its unit', () => {
    expect(splitMetricValue('0 B')).toEqual({ number: '0', unit: 'B' });
    expect(splitMetricValue('1.2 GiB')).toEqual({ number: '1.2', unit: 'GiB' });
    expect(splitMetricValue('12.3K')).toEqual({ number: '12.3', unit: 'K' });
    expect(splitMetricValue('48')).toEqual({ number: '48', unit: '' });
  });

  it('leaves non-numeric placeholders untouched', () => {
    expect(splitMetricValue('—')).toEqual({ number: '—', unit: '' });
  });

  it('round-trips what formatBytesCompact produces', () => {
    const { number, unit } = splitMetricValue(formatBytesCompact(3.4 * 1024 ** 3));
    expect(Number(number)).toBeCloseTo(3.4, 1);
    expect(unit).toBe('GiB');
  });
});
