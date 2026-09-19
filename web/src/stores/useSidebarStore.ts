import { create } from 'zustand';
import { persist } from 'zustand/middleware';

/**
 * Sidebar personalization: user-pinned destinations surfaced above the fixed
 * nav groups. Persisted to localStorage — route ids are stable
 * (`ProductRouteId` in `product/ia.ts`), so we store raw id strings and resolve
 * them to routes at render time.
 */
const MAX_PINNED = 4;

interface SidebarState {
  pinned: string[];
  pin: (id: string) => void;
  unpin: (id: string) => void;
  togglePin: (id: string) => void;
  reorderPinned: (fromId: string, toId: string) => void;
}

export const useSidebarStore = create<SidebarState>()(
  persist(
    (set, get) => ({
      pinned: [],
      pin: (id) =>
        set((s) =>
          s.pinned.includes(id) || s.pinned.length >= MAX_PINNED
            ? s
            : { pinned: [...s.pinned, id] },
        ),
      unpin: (id) => set((s) => ({ pinned: s.pinned.filter((p) => p !== id) })),
      togglePin: (id) => (get().pinned.includes(id) ? get().unpin(id) : get().pin(id)),
      // Move `fromId` to sit just before `toId` (drag-to-reorder). The target
      // index is recomputed after removal so the result is order-stable.
      reorderPinned: (fromId, toId) =>
        set((s) => {
          if (fromId === toId) return s;
          const ids = [...s.pinned];
          const from = ids.indexOf(fromId);
          if (from < 0) return s;
          ids.splice(from, 1);
          const to = ids.indexOf(toId);
          if (to < 0) return { pinned: [...ids, fromId] };
          ids.splice(to, 0, fromId);
          return { pinned: ids };
        }),
    }),
    { name: 'molesignal.sidebar.v1' },
  ),
);
