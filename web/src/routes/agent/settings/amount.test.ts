import { describe, expect, it } from 'vitest';

import { optionalAmount } from './amount';

describe('optionalAmount', () => {
  it('treats an empty field as not set', () => {
    expect(optionalAmount('')).toBeNull();
    expect(optionalAmount('   ')).toBeNull();
  });

  it('reads zero and positive numbers', () => {
    expect(optionalAmount('0')).toBe(0);
    expect(optionalAmount(' 2.5 ')).toBe(2.5);
  });

  it('rejects negative and non-numeric input', () => {
    expect(optionalAmount('-1')).toBe('invalid');
    expect(optionalAmount('abc')).toBe('invalid');
    expect(optionalAmount('Infinity')).toBe('invalid');
  });
});
