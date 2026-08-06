import { create } from 'zustand';
import { persist } from 'zustand/middleware';

/**
 * Collapse state for generic management sub-navigation (account, …).
 * Settings and IAM use independent fully-hidden sidebar preferences because
 * they have dedicated narrow-screen drawers and must not become icon rails.
 */
interface SubnavState {
  collapsed: boolean;
  toggle: () => void;
  setCollapsed: (v: boolean) => void;
}

export const useSubnavStore = create<SubnavState>()(
  persist(
    (set, get) => ({
      collapsed: false,
      toggle: () => set({ collapsed: !get().collapsed }),
      setCollapsed: (v) => set({ collapsed: v }),
    }),
    { name: 'molesignal.subnav.v1' },
  ),
);
