import { describe, expect, it } from 'vitest';

import { rankResults } from '@/palette/fuzzy';

describe('palette fuzzy ranking', () => {
  it('rankResults returns an array', () => {
    // rankResults signature is (query, items). We pass a tiny shape that satisfies
    // ResultItem at compile time via `as unknown as`.
    const items = [
      { id: 'a', label: 'alerts overview', kind: 'action' as const, usedAt: 0 },
    ] as unknown as Parameters<typeof rankResults>[1];
    const out = rankResults('al', items);
    expect(Array.isArray(out)).toBe(true);
  });
});
