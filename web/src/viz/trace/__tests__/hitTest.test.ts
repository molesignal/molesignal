import { describe, expect, it } from 'vitest';

import { HitTester } from '@/viz/trace/hitTest';

describe('HitTester', () => {
  it('class exists with hit method', () => {
    // SpanNode / LaidOutTrace shapes are complex; full hit-region tests live
    // in Playwright (real canvas). Here we assert the class surface.
    expect(typeof HitTester).toBe('function');
    expect(typeof HitTester.prototype.hit).toBe('function');
  });
});
