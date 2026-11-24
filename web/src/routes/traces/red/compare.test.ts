import { describe, expect, it } from 'vitest';

import { choiceParams, formatShare } from './compare';

const total = { requests: 100, errors: 4, p50Ns: 1_000, p95Ns: 9_000.4, p99Ns: 20_000 };

describe('choiceParams', () => {
  it('asks for the failed requests, or those as slow as a percentile', () => {
    expect(choiceParams('failed', total)).toEqual({ errors: true });
    expect(choiceParams('p95', total)).toEqual({ minDuration: 9_000 });
    expect(choiceParams('p99', total)).toEqual({ minDuration: 20_000 });
    expect(choiceParams(null, total)).toBeNull();
  });

  it('asks for nothing the figures cannot tell', () => {
    expect(choiceParams('failed', { ...total, errors: 0 })).toBeNull();
    expect(choiceParams('failed', { ...total, errors: null })).toBeNull();
    expect(choiceParams('p95', { ...total, p95Ns: null })).toBeNull();
  });
});

describe('formatShare', () => {
  it('says a share as a percentage', () => {
    expect(formatShare(0)).toBe('0%');
    expect(formatShare(0.042)).toBe('4.2%');
    expect(formatShare(0.666)).toBe('67%');
  });
});
