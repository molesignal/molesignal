import { create } from 'zustand';

export const SETTINGS_SIDEBAR_STORAGE_KEY = 'settings_sidebar_collapsed';

interface SettingsSidebarState {
  collapsed: boolean;
  toggle: () => void;
  setCollapsed: (collapsed: boolean) => void;
}

function readCollapsed(): boolean {
  if (typeof window === 'undefined') return false;
  return window.localStorage.getItem(SETTINGS_SIDEBAR_STORAGE_KEY) === 'true';
}

function persistCollapsed(collapsed: boolean): void {
  if (typeof window === 'undefined') return;
  window.localStorage.setItem(
    SETTINGS_SIDEBAR_STORAGE_KEY,
    String(collapsed),
  );
}

export const useSettingsSidebarStore = create<SettingsSidebarState>(
  (set, get) => ({
    collapsed: readCollapsed(),
    toggle: () => {
      const collapsed = !get().collapsed;
      persistCollapsed(collapsed);
      set({ collapsed });
    },
    setCollapsed: (collapsed) => {
      persistCollapsed(collapsed);
      set({ collapsed });
    },
  }),
);
