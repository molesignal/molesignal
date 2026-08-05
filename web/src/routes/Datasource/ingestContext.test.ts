import { describe, expect, it } from 'vitest';

import { isValidRumApplicationId } from './ingestContext';

describe('RUM datasource application IDs', () => {
  it('accepts the identifiers supported by credentials and debug artifacts', () => {
    expect(isValidRumApplicationId('checkout-mobile')).toBe(true);
    expect(isValidRumApplicationId('com.example.checkout:ios_1')).toBe(true);
  });

  it('rejects blank, path-like, and oversized identifiers', () => {
    expect(isValidRumApplicationId('')).toBe(false);
    expect(isValidRumApplicationId('../checkout')).toBe(false);
    expect(isValidRumApplicationId('a'.repeat(129))).toBe(false);
  });
});
