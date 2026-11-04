import { describe, expect, it } from 'vitest';

import type { RumAggregate } from '@/api/rum';

import { comparisonQuery, flagField, readVariants } from './model';

describe('a flag is asked by a key of the feature flags', () => {
  it('names the key whole, dots and all', () => {
    expect(flagField('checkout.v2')).toBe('feature_flags.checkout.v2');
  });

  it('compares its values over the views of the application', () => {
    const query = comparisonQuery({ application: 'shop', environment: 'prod' }, { from: 1, to: 2 }, 'checkout');

    expect(query.type).toBe('views');
    expect(query.groupBy).toEqual(['feature_flags.checkout']);
    expect(query.filters).toEqual([
      { field: 'application.id', op: '=', value: 'shop' },
      { field: 'env', op: '=', value: 'prod' },
    ]);
  });
});

describe('readVariants', () => {
  it('says each value by its numbers, and shares of its views', () => {
    const measures = ['sessions', 'views', 'with_error', 'frustrated', 'lcp', 'inp', 'cls'].map((name) => ({ name, fn: 'count', unit: 'count' }));
    const answer = {
      groupBy: ['feature_flags.checkout'],
      measures,
      rows: [
        { group: ['on'], values: [10, 20, 5, 2, 1_000_000, null, 0.1] },
        { group: [null], values: [3, 0, 0, 0, null, null, null] },
      ],
    } as unknown as RumAggregate;

    expect(readVariants(answer)).toEqual([
      { value: 'on', sessions: 10, views: 20, errorShare: 0.25, frustrationShare: 0.1, lcp: 1_000_000, inp: null, cls: 0.1 },
      { value: null, sessions: 3, views: 0, errorShare: null, frustrationShare: null, lcp: null, inp: null, cls: null },
    ]);
  });

  it('has nothing to say without an answer', () => {
    expect(readVariants(undefined)).toEqual([]);
  });
});
