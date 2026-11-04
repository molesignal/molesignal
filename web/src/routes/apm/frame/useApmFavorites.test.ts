import { describe, expect, it } from 'vitest';

import { MAX_FAVORITES, parseFavorites, toggledFavorites } from './useApmFavorites';

describe('APM favorites', () => {
  it('adds a service at the end and takes a favorite out', () => {
    const kept = [{ namespace: 'shop', name: 'checkout' }];

    expect(toggledFavorites(kept, { namespace: 'shop', name: 'cart' })).toEqual([...kept, { namespace: 'shop', name: 'cart' }]);
    expect(toggledFavorites(kept, { namespace: 'shop', name: 'checkout' })).toEqual([]);
  });

  it('keeps no more than the server does, dropping the oldest', () => {
    const many = Array.from({ length: MAX_FAVORITES }, (_, index) => ({ namespace: 'shop', name: `s${index}` }));
    const next = toggledFavorites(many, { namespace: 'shop', name: 'new' });

    expect(next).toHaveLength(MAX_FAVORITES);
    expect(next[0]?.name).toBe('s1');
    expect(next.at(-1)?.name).toBe('new');
  });

  it('reads only what it knows', () => {
    expect(parseFavorites({ services: [{ namespace: 'shop', name: 'cart' }, 'shop/x', { name: 'y' }] })).toEqual({
      services: [{ namespace: 'shop', name: 'cart' }],
    });
    expect(parseFavorites(['shop/cart'])).toBeNull();
    expect(parseFavorites(null)).toBeNull();
  });
});
