import { describe, expect, it } from 'vitest';

import { segmentFilters } from './segment';

describe('a segment as filters', () => {
  it('asks for one of the values, or none of them, of each condition', () => {
    expect(
      segmentFilters({
        id: 'big',
        name: 'Big customers',
        conditions: [
          { field: 'usr.extra.plan', op: 'is', values: ['enterprise', 'scale'] },
          { field: 'geo.country_iso_code', op: 'is_not', values: ['CN'] },
        ],
      }),
    ).toEqual([
      { field: 'usr.extra.plan', op: 'in', value: '["enterprise","scale"]' },
      { field: 'geo.country_iso_code', op: 'not_in', value: '["CN"]' },
    ]);
  });

  it('asks for nothing when there is no segment', () => {
    expect(segmentFilters(undefined)).toEqual([]);
  });
});
