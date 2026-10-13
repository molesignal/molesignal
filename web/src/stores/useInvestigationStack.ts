import { nanoid } from 'nanoid';
import { create } from 'zustand';

import type { TimeWindow } from '@/stores/useTimeStore';

export type FrameKind =
  | 'trace'
  | 'trace_span'
  | 'log'
  | 'log_row'
  | 'metric'
  | 'host'
  | 'service'
  | 'service_to_service'
  | 'incident'
  | 'sql'
  | 'promql'
  | 'dashboard_panel'
  | 'saved_view';

export interface Frame {
  id: string;
  kind: FrameKind;
  params: Record<string, unknown>;
  time_range_override?: TimeWindow;
  anchor_override?: string;
  parent_frame_id?: string;
  pinned: boolean;
  created_at: number;
}

/**
 * A frame as an address names it: what it shows, without the identity it has
 * while it is open. Addresses carry no frame ids.
 */
export type FrameContent = Omit<Frame, 'id' | 'created_at'>;

export const MAX_FRAMES = 6;

interface StackState {
  frames: Frame[];
  forward: Frame[];
  push: (frame: Omit<Frame, 'id' | 'created_at' | 'pinned'> & { pinned?: boolean }) => Frame | null;
  pop: () => void;
  back: () => void;
  forwardOne: () => void;
  reset: () => void;
  pinFrame: (id: string, pinned: boolean) => void;
  popTo: (id: string) => void;
  hydrate: (frames: FrameContent[]) => void;
}

/**
 * Equal for two frames that show the same thing. Whether a frame is pinned is
 * left out: pinning changes how long it is kept, not what it is.
 */
function contentKey(frame: FrameContent): string {
  return JSON.stringify([
    frame.kind,
    frame.params,
    frame.time_range_override,
    frame.anchor_override,
    frame.parent_frame_id,
  ]);
}

export const useInvestigationStack = create<StackState>((set, get) => ({
  frames: [],
  forward: [],
  push: (incoming) => {
    const { frames } = get();
    const newFrame: Frame = {
      ...incoming,
      pinned: incoming.pinned ?? false,
      id: nanoid(10),
      created_at: Date.now(),
    };
    if (frames.length >= MAX_FRAMES) {
      // drop oldest unpinned; refuse if all pinned
      const dropIdx = frames.findIndex((f) => !f.pinned);
      if (dropIdx < 0) {
        return null;
      }
      const next = [...frames.slice(0, dropIdx), ...frames.slice(dropIdx + 1), newFrame];
      set({ frames: next, forward: [] });
    } else {
      set({ frames: [...frames, newFrame], forward: [] });
    }
    return newFrame;
  },
  pop: () => {
    const { frames } = get();
    if (frames.length === 0) return;
    set({ frames: frames.slice(0, -1), forward: [] });
  },
  back: () => {
    const { frames, forward } = get();
    if (frames.length === 0) return;
    const top = frames[frames.length - 1]!;
    set({ frames: frames.slice(0, -1), forward: [...forward, top] });
  },
  forwardOne: () => {
    const { frames, forward } = get();
    if (forward.length === 0) return;
    const restore = forward[forward.length - 1]!;
    set({ frames: [...frames, restore], forward: forward.slice(0, -1) });
  },
  reset: () => set({ frames: [], forward: [] }),
  pinFrame: (id, pinned) =>
    set({ frames: get().frames.map((f) => (f.id === id ? { ...f, pinned } : f)) }),
  popTo: (id) => {
    const { frames } = get();
    const idx = frames.findIndex((f) => f.id === id);
    if (idx < 0) return;
    const popped = frames.slice(idx + 1).reverse();
    set({ frames: frames.slice(0, idx + 1), forward: popped });
  },
  // Called on every address change, and the stack writes itself to the
  // address, so most calls name exactly the frames that are already open.
  // Those keep their identity: under a new id a frame is unmounted and mounted
  // again, which shows its drawer leaving and fetches its data a second time.
  hydrate: (named) => {
    const { frames } = get();
    const openKeys = frames.map(contentKey);
    // Matched in order, so an address that drops frames from either end
    // leaves the ones it keeps as they are.
    let from = 0;
    const next = named.map((content): Frame => {
      const key = contentKey(content);
      const at = openKeys.findIndex((openKey, i) => i >= from && openKey === key);
      if (at < 0) return { ...content, id: nanoid(10), created_at: Date.now() };

      from = at + 1;
      const open = frames[at]!;
      return open.pinned === content.pinned ? open : { ...open, pinned: content.pinned };
    });
    if (next.length === frames.length && next.every((frame, i) => frame === frames[i])) return;
    set({ frames: next, forward: [] });
  },
}));
