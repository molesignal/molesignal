import fc from 'fast-check';
import { beforeEach, describe, expect, it } from 'vitest';

import { MAX_FRAMES, useInvestigationStack, type FrameKind } from '@/stores/useInvestigationStack';

const KINDS: FrameKind[] = ['trace', 'log', 'metric', 'host', 'service'];

describe('useInvestigationStack', () => {
  beforeEach(() => useInvestigationStack.getState().reset());

  it('push respects MAX_FRAMES, dropping oldest unpinned', () => {
    const { push } = useInvestigationStack.getState();
    for (let i = 0; i < MAX_FRAMES + 2; i++) {
      push({ kind: 'trace', params: { i } });
    }
    const frames = useInvestigationStack.getState().frames;
    expect(frames.length).toBe(MAX_FRAMES);
    // oldest two (i=0,1) should have been dropped
    expect(frames[0]!.params).not.toEqual({ i: 0 });
  });

  it('refuses push when MAX_FRAMES all pinned', () => {
    const { push, pinFrame } = useInvestigationStack.getState();
    for (let i = 0; i < MAX_FRAMES; i++) {
      const f = push({ kind: 'trace', params: { i } });
      if (f) pinFrame(f.id, true);
    }
    const refused = push({ kind: 'log', params: { i: 'overflow' } });
    expect(refused).toBeNull();
    expect(useInvestigationStack.getState().frames.length).toBe(MAX_FRAMES);
  });

  it('back/forward invariants: composition is identity on top frame', () => {
    const { push, back, forwardOne } = useInvestigationStack.getState();
    push({ kind: 'trace', params: { a: 1 } });
    push({ kind: 'log', params: { b: 2 } });
    const beforeTop = useInvestigationStack.getState().frames.at(-1)!;
    back();
    forwardOne();
    const afterTop = useInvestigationStack.getState().frames.at(-1)!;
    expect(afterTop).toEqual(beforeTop);
  });

  it('property: random push/pop sequences keep frames.length within [0, MAX_FRAMES]', () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.oneof(
            fc.record({ op: fc.constant('push' as const), kind: fc.constantFrom(...KINDS) }),
            fc.record({ op: fc.constant('pop' as const) }),
          ),
          { maxLength: 50 },
        ),
        (ops) => {
          useInvestigationStack.getState().reset();
          for (const op of ops) {
            if (op.op === 'push') useInvestigationStack.getState().push({ kind: op.kind, params: {} });
            else useInvestigationStack.getState().pop();
            const n = useInvestigationStack.getState().frames.length;
            expect(n).toBeGreaterThanOrEqual(0);
            expect(n).toBeLessThanOrEqual(MAX_FRAMES);
          }
        },
      ),
      { numRuns: 100 },
    );
  });
});
