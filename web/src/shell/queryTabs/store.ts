import { create } from 'zustand';
import { persist } from 'zustand/middleware';

import { createTimeStore, type TimeStoreApi } from '@/stores/useTimeStore';

import {
  activateQueryTab,
  addQueryTab,
  closeOtherQueryTabs,
  closeQueryTab,
  duplicateQueryTab,
  moveQueryTabAcross,
  openQueryTabs,
  type QueryTab,
  type QueryTabsArrival,
  type QueryTabsKind,
  type QueryTabsState,
  type QueryTabTime,
  rememberInQueryTab,
  renameQueryTab,
  setQueryPaneCount,
  setQueryTabSearch,
  setQueryTabSummary,
  setQueryTabTime,
} from './model';

export const QUERY_TABS_STORAGE_KEY = 'molesignal.query-tabs.v1';

/**
 * Tabs are kept per user, organization and explorer: a query names streams and
 * services of one organization, and one browser may be shared by several users.
 */
export function queryTabsScope(userId: string, orgId: string, kind: QueryTabsKind): string {
  return `${userId}:${orgId}:${kind}`;
}

interface QueryTabsStore {
  scopes: Record<string, QueryTabsState>;
  open: (scope: string, arrival: QueryTabsArrival) => void;
  /** Opens a new tab after `besideId`, in its half of the page; after the tab in front when none is named. */
  add: (scope: string, besideId?: string) => void;
  duplicate: (scope: string, id: string) => void;
  activate: (scope: string, id: string) => void;
  close: (scope: string, id: string) => void;
  closeOthers: (scope: string, id: string) => void;
  /** Splits the page in two halves (2) or gives it back to one tab (1). */
  setPaneCount: (scope: string, count: number) => void;
  /** Moves a tab to the other half of a split page. */
  moveAcross: (scope: string, id: string) => void;
  rename: (scope: string, id: string, label: string) => void;
  setSummary: (scope: string, id: string, summary: string) => void;
  setSearch: (scope: string, id: string, search: string) => void;
  setTime: (scope: string, id: string, time: QueryTabTime) => void;
  remember: (scope: string, id: string, key: string, value: unknown) => void;
}

/**
 * The open query tabs of every explorer. Only what brings a tab back is saved
 * (its query, time range and kept state); results stay with the mounted page.
 */
export const useQueryTabsStore = create<QueryTabsStore>()(
  persist(
    (set) => {
      const change = (
        scope: string,
        next: (state: QueryTabsState) => QueryTabsState,
      ) =>
        set((store) => {
          const current = store.scopes[scope];
          if (!current) return store;
          const updated = next(current);
          return updated === current
            ? store
            : { scopes: { ...store.scopes, [scope]: updated } };
        });

      return {
        scopes: {},
        open: (scope, arrival) =>
          set((store) => ({
            scopes: {
              ...store.scopes,
              [scope]: openQueryTabs(store.scopes[scope], arrival, Date.now()),
            },
          })),
        add: (scope, besideId) => change(scope, (state) => addQueryTab(state, Date.now(), besideId)),
        duplicate: (scope, id) =>
          change(scope, (state) => duplicateQueryTab(state, id, Date.now())),
        activate: (scope, id) =>
          change(scope, (state) => activateQueryTab(state, id, Date.now())),
        close: (scope, id) => change(scope, (state) => closeQueryTab(state, id, Date.now())),
        closeOthers: (scope, id) =>
          change(scope, (state) => closeOtherQueryTabs(state, id, Date.now())),
        setPaneCount: (scope, count) =>
          change(scope, (state) => setQueryPaneCount(state, count, Date.now())),
        moveAcross: (scope, id) =>
          change(scope, (state) => moveQueryTabAcross(state, id, Date.now())),
        rename: (scope, id, label) => change(scope, (state) => renameQueryTab(state, id, label)),
        setSummary: (scope, id, summary) =>
          change(scope, (state) => setQueryTabSummary(state, id, summary)),
        setSearch: (scope, id, search) =>
          change(scope, (state) => setQueryTabSearch(state, id, search)),
        setTime: (scope, id, time) => change(scope, (state) => setQueryTabTime(state, id, time)),
        remember: (scope, id, key, value) =>
          change(scope, (state) => rememberInQueryTab(state, id, key, value)),
      };
    },
    {
      name: QUERY_TABS_STORAGE_KEY,
      version: 1,
      partialize: (store) => ({ scopes: store.scopes }),
    },
  ),
);

/** Which tabs have a query in flight. Not saved: a reload starts with none running. */
export const useQueryTabsRuntime = create<{
  running: Record<string, boolean>;
  setRunning: (id: string, running: boolean) => void;
}>()((set) => ({
  running: {},
  setRunning: (id, running) =>
    set((state) => {
      if (Boolean(state.running[id]) === running) return state;
      const next = { ...state.running };
      if (running) next[id] = true;
      else delete next[id];
      return { running: next };
    }),
}));

const timeStores = new Map<string, TimeStoreApi>();

/** A tab's key outside its own set of tabs: ids are only unique within one scope. */
export function queryTabKey(scope: string, id: string): string {
  return `${scope}/${id}`;
}

/** The time store of a tab, made from the tab's saved range the first time it is asked for. */
export function queryTabTimeStore(
  scope: string,
  tab: Pick<QueryTab, 'id' | 'window' | 'anchor'>,
): TimeStoreApi {
  const key = queryTabKey(scope, tab.id);
  let store = timeStores.get(key);
  if (!store) {
    store = createTimeStore({ window: tab.window, anchor: tab.anchor });
    timeStores.set(key, store);
  }
  return store;
}

/** Forgets the time stores of tabs that are gone. */
export function pruneQueryTabTimeStores(scope: string, liveIds: ReadonlySet<string>): void {
  const prefix = queryTabKey(scope, '');
  for (const key of timeStores.keys()) {
    if (key.startsWith(prefix) && !liveIds.has(key.slice(prefix.length))) timeStores.delete(key);
  }
}
