import { describe, expect, it } from 'vitest';

import { SPAN_SHARE_FILL, SPAN_SHARE_STEPS, spanShare } from './model';

describe('spanShare', () => {
  it.each([
    [0, 'minor'],
    [24.9, 'minor'],
    [25, 'notable'],
    [49.9, 'notable'],
    [50, 'major'],
    [74.9, 'major'],
    [75, 'dominant'],
    [100, 'dominant'],
  ] as const)('puts a span that took %s%% of the trace in the %s step', (percent, share) => {
    expect(spanShare(percent, 100)).toBe(share);
  });

  it('measures against the trace, not against a fixed duration', () => {
    // The same 40 ms is most of a short trace and little of a long one.
    expect(spanShare(40_000_000, 50_000_000)).toBe('dominant');
    expect(spanShare(40_000_000, 2_000_000_000)).toBe('minor');
  });

  it('keeps a span that outlasts the trace in the last step, and a trace of no length in the first', () => {
    expect(spanShare(300, 200)).toBe('dominant');
    expect(spanShare(10, 0)).toBe('minor');
    expect(spanShare(Number.NaN, 100)).toBe('minor');
  });

  it('has a fill of its own for every step, in order from the least', () => {
    expect(SPAN_SHARE_STEPS.map((step) => step.from)).toEqual([0, 25, 50, 75]);

    const fills = SPAN_SHARE_STEPS.map((step) => SPAN_SHARE_FILL[step.share]);
    expect(new Set(fills).size).toBe(SPAN_SHARE_STEPS.length);
    // Red is a failed span's, not a step of the scale.
    expect(fills.some((fill) => fill.includes('red'))).toBe(false);
  });
});
