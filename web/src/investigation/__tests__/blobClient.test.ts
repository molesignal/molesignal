import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import { shouldUseBlob } from '@/investigation/blobClient';
import { decodeStack, encodeStack, hydrateFromSearchParams } from '@/shell/UrlHydration';
import { useInvestigationStack, type Frame } from '@/stores/useInvestigationStack';

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

  // The stack writes itself to the address after every change and the address
  // is read back into the stack. A frame that came back from that trip under a
  // new identity would be unmounted and mounted again.
  it('reading back the address the stack wrote leaves the open frames as they are', () => {
    const pushArb = fc.record(
      {
        kind: fc.constantFrom('trace', 'log', 'metric', 'host', 'service'),
        params: fc.dictionary(fc.string({ maxLength: 12 }), fc.option(fc.jsonValue(), { nil: undefined })),
        time_range_override: fc.record({
          from: fc.string({ maxLength: 12 }),
          to: fc.string({ maxLength: 12 }),
          mode: fc.constantFrom('relative', 'absolute'),
        }),
        anchor_override: fc.string({ maxLength: 12 }),
        parent_frame_id: fc.string({ maxLength: 10 }),
        pinned: fc.boolean(),
      },
      { requiredKeys: ['kind', 'params'] },
    ) as fc.Arbitrary<Parameters<ReturnType<typeof useInvestigationStack.getState>['push']>[0]>;

    fc.assert(
      fc.property(fc.array(pushArb, { maxLength: 6 }), (pushes) => {
        useInvestigationStack.getState().reset();
        for (const frame of pushes) useInvestigationStack.getState().push(frame);
        const open = useInvestigationStack.getState().frames;

        hydrateFromSearchParams(new URLSearchParams({ stack: encodeStack(open) }));

        expect(useInvestigationStack.getState().frames).toBe(open);
      }),
      { numRuns: 100 },
    );
  });
});
