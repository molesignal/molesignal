import fc from 'fast-check';
import { beforeEach, describe, expect, it } from 'vitest';

import {
  MAX_FRAMES,
  useInvestigationStack,
  type Frame,
  type FrameContent,
  type FrameKind,
} from '@/stores/useInvestigationStack';

const KINDS: FrameKind[] = ['trace', 'log', 'metric', 'host', 'service'];

/** A frame the way an address names it, after the trip through JSON. */
function named(frame: Frame, change: Partial<FrameContent> = {}): FrameContent {
  const { id: _id, created_at: _createdAt, ...content } = frame;
  return { ...(JSON.parse(JSON.stringify(content)) as FrameContent), ...change };
}

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

  describe('hydrate', () => {
    const service = (name: string): FrameContent => ({
      kind: 'service',
      params: { service: name },
      pinned: false,
    });

    it('opens the frames an address names, each under an id of its own', () => {
      useInvestigationStack.getState().hydrate([service('web'), service('web')]);

      const frames = useInvestigationStack.getState().frames;
      expect(frames.map((frame) => frame.params)).toEqual([{ service: 'web' }, { service: 'web' }]);
      expect(frames[0]!.id).not.toBe('');
      expect(frames[0]!.id).not.toBe(frames[1]!.id);
    });

    it('changes nothing when the address names the frames that are already open', () => {
      const { push, back, hydrate } = useInvestigationStack.getState();
      const web = push({ kind: 'service', params: { service: 'web' } })!;
      const api = push({ kind: 'service', params: { service: 'api' }, parent_frame_id: web.id })!;
      push({ kind: 'log', params: { stream: 'logs', row: undefined } });
      back();
      const before = useInvestigationStack.getState();
      let notified = 0;
      const unsubscribe = useInvestigationStack.subscribe(() => {
        notified += 1;
      });

      hydrate([named(web), named(api)]);
      unsubscribe();

      // The same array and the same objects: nothing that reads them runs again.
      expect(useInvestigationStack.getState().frames).toBe(before.frames);
      expect(notified).toBe(0);
      // A frame stepped back from can still be stepped forward to.
      expect(useInvestigationStack.getState().forward).toBe(before.forward);
    });

    it('keeps the frames an address still names and lets the others go', () => {
      const { push, hydrate } = useInvestigationStack.getState();
      const web = push({ kind: 'service', params: { service: 'web' } })!;
      const api = push({ kind: 'service', params: { service: 'api' } })!;
      const db = push({ kind: 'service', params: { service: 'db' } })!;

      hydrate([named(web), named(api)]);
      expect(useInvestigationStack.getState().frames).toEqual([web, api]);
      expect(useInvestigationStack.getState().frames[1]).toBe(api);

      useInvestigationStack.setState({ frames: [web, api, db] });
      hydrate([named(api), named(db)]);
      expect(useInvestigationStack.getState().frames[0]).toBe(api);
      expect(useInvestigationStack.getState().frames[1]).toBe(db);
    });

    it('opens what the address adds and keeps what was open', () => {
      const { push, back, hydrate } = useInvestigationStack.getState();
      const web = push({ kind: 'service', params: { service: 'web' } })!;
      push({ kind: 'service', params: { service: 'api' } });
      back();

      hydrate([named(web), service('db')]);

      const { frames, forward } = useInvestigationStack.getState();
      expect(frames[0]).toBe(web);
      expect(frames[1]!.params).toEqual({ service: 'db' });
      expect(frames[1]!.id).not.toBe(web.id);
      // The address moved the stack somewhere else, so there is no step forward left.
      expect(forward).toEqual([]);
    });

    it('takes the pin from the address and leaves the frame its id', () => {
      const { push, hydrate } = useInvestigationStack.getState();
      const web = push({ kind: 'service', params: { service: 'web' } })!;

      hydrate([named(web, { pinned: true })]);

      expect(useInvestigationStack.getState().frames).toEqual([{ ...web, pinned: true }]);
    });

    it('tells two open frames with the same content apart by their place', () => {
      const { push, hydrate } = useInvestigationStack.getState();
      const first = push({ kind: 'service', params: { service: 'web' } })!;
      const second = push({ kind: 'service', params: { service: 'web' } })!;

      hydrate([named(first), named(second)]);
      expect(useInvestigationStack.getState().frames[0]).toBe(first);
      expect(useInvestigationStack.getState().frames[1]).toBe(second);

      hydrate([named(second)]);
      expect(useInvestigationStack.getState().frames).toEqual([first]);
    });
  });
});
