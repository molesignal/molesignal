import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import { shouldUseBlob } from '@/investigation/blobClient';
import { decodeStack, encodeStack } from '@/shell/UrlHydration';
import type { Frame } from '@/stores/useInvestigationStack';

describe('investigation blob client', () => {
  it('flags payload > 4 KiB to use blob endpoint', () => {
    expect(shouldUseBlob('x')).toBe(false);
    expect(shouldUseBlob('x'.repeat(5 * 1024))).toBe(true);
  });
});

describe('stack encode/decode property', () => {
  const frameArb = fc.record({
    id: fc.string({ minLength: 1, maxLength: 16 }),
    kind: fc.constantFrom('trace', 'log', 'metric', 'host', 'service'),
    params: fc.record({
      key: fc.string({ maxLength: 40 }),
    }),
    pinned: fc.boolean(),
    created_at: fc.integer({ min: 0, max: 2 ** 32 }),
  }) as fc.Arbitrary<Frame>;

  it('round-trips a stack through encodeStack/decodeStack', () => {
    fc.assert(
      fc.property(fc.array(frameArb, { maxLength: 6 }), (frames) => {
        const encoded = encodeStack(frames);
        const decoded = decodeStack(encoded);
        expect(decoded.length).toBe(frames.length);
        for (let i = 0; i < frames.length; i++) {
          expect(decoded[i]!.kind).toBe(frames[i]!.kind);
        }
      }),
      { numRuns: 50 },
    );
  });
});
