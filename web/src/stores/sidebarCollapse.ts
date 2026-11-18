import { create } from 'zustand';

interface SidebarCollapseState {
  collapsed: boolean;
  toggle: () => void;
}

/**
 * Whether a module's menu of pages is folded away, kept per browser under `storageKey`: a wide
 * page (a timetable, a table of many columns) is read with the menu folded, and stays so.
 */
export function createSidebarCollapseStore(storageKey: string) {
  const read = (): boolean => {
    try {
      return window.localStorage.getItem(storageKey) === 'true';
    } catch {
      return false;
    }
  };
  const keep = (collapsed: boolean): void => {
    try {
      window.localStorage.setItem(storageKey, String(collapsed));
    } catch {
      // A browser that keeps nothing folds the menu for this visit only.
    }
  };

  return create<SidebarCollapseState>((set, get) => ({
    collapsed: typeof window === 'undefined' ? false : read(),
    toggle: () => {
      const collapsed = !get().collapsed;
      keep(collapsed);
      set({ collapsed });
    },
  }));
}

/** The menu of the Alerts module's pages. */
export const useAlertsSidebarStore = createSidebarCollapseStore('alerts_sidebar_collapsed');
