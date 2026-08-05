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
  hydrate: (frames: Frame[]) => void;
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
  hydrate: (frames) => set({ frames, forward: [] }),
}));
