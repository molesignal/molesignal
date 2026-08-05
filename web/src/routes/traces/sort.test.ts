import { describe, expect, it } from 'vitest';

import { parseTraceListSort, writeTraceListSort } from './sort';

describe('trace list sort URL state', () => {
  it('defaults missing and invalid values to latest first', () => {
    expect(parseTraceListSort(null)).toBe('latest');
    expect(parseTraceListSort('not-a-sort')).toBe('latest');
  });

  it('round-trips explicit investigation sorts and omits the default', () => {
    const slowest = writeTraceListSort(
      new URLSearchParams('service=checkout'),
      'duration_desc',
    );
    expect(slowest.get('sort')).toBe('duration_desc');
    expect(parseTraceListSort(slowest.get('sort'))).toBe('duration_desc');

    const latest = writeTraceListSort(slowest, 'latest');
    expect(latest.has('sort')).toBe(false);
    expect(latest.get('service')).toBe('checkout');
  });
});
