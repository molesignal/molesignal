import { create } from 'zustand';

/**
 * Global investigation filters — arbitrary `key=value` constraints the
 * operator pins (e.g. env=production, service=payment). They:
 *   1. stay visible + removable in the InvestigationContextBar,
 *   2. ride along every cross-signal jump (SignalReference appends them to the
 *      generated logs / traces / metrics query *and* to the URL),
 *   3. round-trip through `?filters=` so a shared link or reload restores them
 *      (see `shell/UrlHydration.ts`).
 *
 * This is the Principle #3 "continuity across signals" enhancement: a filter
 * set once follows the operator across pages instead of being retyped.
 */
export interface GlobalFilter {
  key: string;
  value: string;
  operator?: '=' | '!=';
}

interface FiltersState {
  filters: GlobalFilter[];
  setFilter: (key: string, value: string, operator?: '=' | '!=') => void;
  removeFilter: (key: string) => void;
  clearFilters: () => void;
  setAll: (filters: GlobalFilter[]) => void;
}

export const useFiltersStore = create<FiltersState>((set) => ({
  filters: [],
  // Last write wins per key, appended to the end so chip order is stable.
  setFilter: (key, value, operator = '=') =>
    set((s) => ({
      filters: [...s.filters.filter((f) => f.key !== key), { key, value, operator }],
    })),
  removeFilter: (key) => set((s) => ({ filters: s.filters.filter((f) => f.key !== key) })),
  clearFilters: () => set({ filters: [] }),
  setAll: (filters) => set({ filters }),
}));
