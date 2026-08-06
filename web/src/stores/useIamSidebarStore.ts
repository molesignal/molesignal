import { create } from 'zustand';

export const IAM_SIDEBAR_STORAGE_KEY = 'iam_sidebar_collapsed';

interface IamSidebarState {
  collapsed: boolean;
  toggle: () => void;
  setCollapsed: (collapsed: boolean) => void;
}

function readCollapsed(): boolean {
  if (typeof window === 'undefined') return false;
  return window.localStorage.getItem(IAM_SIDEBAR_STORAGE_KEY) === 'true';
}

function persistCollapsed(collapsed: boolean): void {
  if (typeof window === 'undefined') return;
  window.localStorage.setItem(IAM_SIDEBAR_STORAGE_KEY, String(collapsed));
}

export const useIamSidebarStore = create<IamSidebarState>((set, get) => ({
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
}));
