import { create } from 'zustand';
import { persist } from 'zustand/middleware';

/**
 * Collapse state for the second-level sub-nav shared by every `SettingsPage`
 * (Settings, IAM, …). One process-wide flag, persisted to localStorage, so
 * collapsing the menu on one admin page keeps it collapsed across them all.
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
