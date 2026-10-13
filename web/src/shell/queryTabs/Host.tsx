import * as React from 'react';
import { useTranslation } from 'react-i18next';
import {
  createSearchParams,
  type SetURLSearchParams,
  useLocation,
  useNavigate,
  useNavigationType,
} from 'react-router-dom';
import { useShallow } from 'zustand/react/shallow';

import { cn } from '@/shell/lib/cn';
import { timeFromSearchParams } from '@/shell/UrlHydration';
import { useAuthStore } from '@/stores/auth';
import {
  sameTimeWindow,
  TimeScopeContext,
  type TimeState,
  useTimeStore,
} from '@/stores/useTimeStore';

import {
  composeSearch,
  MAX_QUERY_TABS,
  ownSearch,
  type QueryTab,
  queryTabName,
  queryTabPanes,
  queryTabSides,
  type QueryTabsKind,
  tabSearchParams,
} from './model';
import {
  QueryPaneContext,
  type QueryTabScope,
  QueryTabScopeContext,
  type QueryTabStatus,
} from './scope';
import { SplitToggle } from './SplitToggle';
import {
  pruneQueryTabTimeStores,
  queryTabKey,
  queryTabsScope,
  queryTabTimeStore,
  useQueryTabsRuntime,
  useQueryTabsStore,
} from './store';
import { QueryTabStrip } from './Strip';

/**
 * Runs an explorer as a set of tabs, each a query of its own.
 *
 * `children` is the explorer page. It is mounted once per tab that has been
 * opened and stays mounted behind the others, so a tab keeps its results, its
 * scroll position and a query still in flight while another tab is in front.
 * What usually ties a page to the rest of the app is given to each copy
 * separately: its search params (`useQueryTabSearchParams`) and its time range
 * (`useTimeStore`, through `TimeScopeContext`). The tab in front is the one the
 * address bar and the app's time range describe, so links, reloads and the
 * other pages keep working as they do without tabs.
 */
export function QueryTabs({
  kind,
  children,
}: {
  kind: QueryTabsKind;
  children: React.ReactNode;
}) {
  const userId = useAuthStore((state) => state.ctx?.user_id ?? '');
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const scope = queryTabsScope(userId, orgId, kind);

  // Another user or organization has other tabs: start over rather than carry state across.
  return (
    <QueryTabsHost key={scope} scope={scope} kind={kind}>
      {children}
    </QueryTabsHost>
  );
}

function sameTime(a: Pick<TimeState, 'window' | 'anchor'>, b: Pick<TimeState, 'window' | 'anchor'>) {
  return sameTimeWindow(a.window, b.window) && a.anchor?.at === b.anchor?.at;
}

function QueryTabsHost({
  scope,
  kind,
  children,
}: {
  scope: string;
  kind: QueryTabsKind;
  children: React.ReactNode;
}) {
  const { t } = useTranslation('common');
  const location = useLocation();
  const navigate = useNavigate();
  const navigationType = useNavigationType();
  const state = useQueryTabsStore((store) => store.scopes[scope]);
  const running = useQueryTabsRuntime((runtime) => runtime.running);
  const [ready, setReady] = React.useState(false);

  // How the page was reached decides which tab comes forward; a later
  // navigation on the page is not an arrival.
  const arrival = React.useRef({
    search: location.search,
    entry: navigationType === 'POP' ? ('load' as const) : ('link' as const),
  }).current;

  React.useLayoutEffect(() => {
    const search = new URLSearchParams(arrival.search);
    const app = useTimeStore.getState();
    useQueryTabsStore.getState().open(scope, {
      own: ownSearch(search),
      time: timeFromSearchParams(search),
      currentTime: { window: app.window, anchor: app.anchor },
      entry: arrival.entry,
    });
    setReady(true);
  }, [arrival, scope]);

  const tabs = React.useMemo(() => (ready ? state?.tabs ?? [] : []), [ready, state?.tabs]);
  const activeId = ready ? state?.activeId : undefined;
  const active = tabs.find((tab) => tab.id === activeId);
  const tabIds = tabs.map((tab) => tab.id).join(' ');

  // ── split: the page in two halves, where the window has room for two ────
  const pagesRef = React.useRef<HTMLDivElement>(null);
  const [room, setRoom] = React.useState(true);
  // The pages are drawn once there is a tab in front.
  const drawn = ready && activeId !== undefined;
  React.useLayoutEffect(() => {
    const pages = pagesRef.current;
    if (!drawn || !pages || typeof ResizeObserver === 'undefined') return;

    const measure = () => setRoom(pages.clientWidth >= 2 * MIN_PANE_WIDTH);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(pages);
    return () => observer.disconnect();
  }, [drawn]);

  // The tab shown in each half, and the tabs each half lists. A window too narrow for two
  // halves shows the tab in front alone, with every tab in its one strip.
  const panes = React.useMemo(() => {
    if (!ready || !state || !activeId) return [];
    return room ? queryTabPanes(state) : [activeId];
  }, [activeId, ready, room, state]);
  const sides = React.useMemo<QueryTab[][]>(
    () => (state && panes.length > 1 ? queryTabSides(state) : [tabs]),
    [panes.length, state, tabs],
  );

  // ── time: the tab in front and the app share one range ──────────────────
  const frontTab = React.useCallback(() => {
    const current = useQueryTabsStore.getState().scopes[scope];
    return current?.tabs.find((tab) => tab.id === current.activeId);
  }, [scope]);

  const showFrontTime = React.useCallback(() => {
    const tab = frontTab();
    if (!tab) return;
    const time = queryTabTimeStore(scope, tab).getState();
    if (!sameTime(useTimeStore.getState(), time)) {
      useTimeStore.setState({ window: time.window, anchor: time.anchor });
    }
  }, [frontTab, scope]);

  React.useEffect(() => {
    if (!ready) return;
    const current = useQueryTabsStore.getState().scopes[scope]?.tabs ?? [];
    pruneQueryTabTimeStores(scope, new Set(current.map((tab) => tab.id)));

    const unsubscribes = current.map((tab) => {
      const store = queryTabTimeStore(scope, tab);
      const publish = (time: TimeState) => {
        useQueryTabsStore
          .getState()
          .setTime(scope, tab.id, { window: time.window, anchor: time.anchor });
        if (frontTab()?.id === tab.id && !sameTime(useTimeStore.getState(), time)) {
          useTimeStore.setState({ window: time.window, anchor: time.anchor });
        }
      };
      // A page may have set its range while mounting, before anyone listened.
      publish(store.getState());
      return store.subscribe(publish);
    });

    // A range set outside the tabs (the shell's picker, the palette, the URL)
    // is meant for what is on screen: the tab in front.
    unsubscribes.push(
      useTimeStore.subscribe((app) => {
        const tab = frontTab();
        if (!tab) return;
        const store = queryTabTimeStore(scope, tab);
        if (!sameTime(store.getState(), app)) {
          store.setState({ window: app.window, anchor: app.anchor });
        }
      }),
    );
    return () => {
      for (const unsubscribe of unsubscribes) unsubscribe();
    };
  }, [frontTab, ready, scope, tabIds]);

  // ── address bar: a mirror of the tab in front ───────────────────────────
  const mirror = React.useRef<{
    /** The tab and params the address bar was last reconciled with. */
    seen: string | null;
    /** What was asked of the router and has not shown up yet. */
    pending: string | null;
    askedAt: string | null;
    replace: boolean;
  }>({ seen: null, pending: null, askedAt: null, replace: true });

  const activeSearch = active?.search;
  React.useEffect(() => {
    if (!activeId || activeSearch === undefined) return;
    const book = mirror.current;
    const shown = ownSearch(location.search);
    const wanted = `${activeId}\n${activeSearch}`;

    const ask = () => {
      const tab = frontTab();
      if (!tab) return;
      const search = composeSearch(
        activeSearch,
        location.search,
        queryTabTimeStore(scope, tab).getState(),
      );
      book.pending = wanted;
      book.askedAt = location.key;
      navigate(
        { pathname: location.pathname, search: search ? `?${search}` : '', hash: location.hash },
        { replace: book.replace, state: location.state },
      );
      book.replace = true;
    };

    if (shown === activeSearch) {
      book.pending = null;
      book.seen = wanted;
      return;
    }
    if (book.pending !== null) {
      // Asked and not there: still on its way, or a write that started before
      // it landed on top. Ask again once the address has moved without us.
      if (book.pending !== wanted || book.askedAt !== location.key) ask();
      book.seen = wanted;
      return;
    }
    if (book.seen !== wanted) {
      // The tab changed (another one came forward, or the page wrote its params).
      book.seen = wanted;
      ask();
      return;
    }
    // The address changed under an unchanged tab: a link, the history, or the
    // shell. It is about what is on screen, so the tab in front takes it.
    useQueryTabsStore.getState().setSearch(scope, activeId, shown);
  }, [
    activeId,
    activeSearch,
    frontTab,
    location.hash,
    location.key,
    location.pathname,
    location.search,
    location.state,
    navigate,
    scope,
  ]);

  // ── tabs ────────────────────────────────────────────────────────────────
  // A tab mounts its page the first time it comes forward, so tabs restored
  // after a reload do not all query at once.
  const opened = React.useRef(new Set<string>()).current;
  if (activeId) opened.add(activeId);
  // A tab that stands beside the one in front is on the page too.
  for (const id of panes) opened.add(id);
  for (const id of opened) {
    if (!tabs.some((tab) => tab.id === id)) opened.delete(id);
  }

  const act = React.useCallback(
    (change: (store: ReturnType<typeof useQueryTabsStore.getState>) => void) => {
      change(useQueryTabsStore.getState());
      // Whichever tab is in front now, the app is on its time range.
      showFrontTime();
    },
    [showFrontTime],
  );

  const onParamsWrite = React.useCallback((replace: boolean) => {
    mirror.current.replace = replace;
  }, []);

  // On a split page the tab that is touched is the one worked in: it comes to the front.
  const workIn = React.useCallback(
    (id: string) => act((store) => store.activate(scope, id)),
    [act, scope],
  );

  if (!ready || !activeId) {
    return <div data-query-tabs={kind} className={HOST_CLASS} />;
  }

  const split = panes.length > 1;

  return (
    <div data-query-tabs={kind} className={HOST_CLASS}>
      {/* A strip over each half of a split page, listing that half's tabs; one strip otherwise. */}
      <div data-query-tab-strips={sides.length} className="flex shrink-0">
        {sides.map((listed, side) => {
          const shownId = panes[side] ?? activeId;

          return (
            <div key={side} className="min-w-0 flex-1 basis-0" style={paneGutters(side, sides.length)}>
              <QueryTabStrip
                tabs={listed.map((tab) => ({
                  id: tab.id,
                  name: queryTabName(tab),
                  ordinal: tab.ordinal,
                  running: Boolean(running[queryTabKey(scope, tab.id)]),
                  tabId: tabElementId(tab.id),
                  panelId: panelElementId(tab.id),
                }))}
                activeId={shownId}
                focused={shownId === activeId}
                closable={tabs.length > 1}
                canAdd={tabs.length < MAX_QUERY_TABS}
                limit={MAX_QUERY_TABS}
                moveAcross={
                  split
                    ? {
                        label: t(side === 0 ? 'query_tabs.move_right' : 'query_tabs.move_left'),
                        onMove: (id) => act((store) => store.moveAcross(scope, id)),
                      }
                    : undefined
                }
                end={
                  side === sides.length - 1 ? (
                    <SplitToggle
                      split={split}
                      room={room}
                      onChange={(next) => act((store) => store.setPaneCount(scope, next ? 2 : 1))}
                    />
                  ) : undefined
                }
                onSelect={(id) => act((store) => store.activate(scope, id))}
                onAdd={() => act((store) => store.add(scope, shownId))}
                onClose={(id) => act((store) => store.close(scope, id))}
                onCloseOthers={(id) => act((store) => store.closeOthers(scope, id))}
                onDuplicate={(id) => act((store) => store.duplicate(scope, id))}
                onRename={(id, label) => useQueryTabsStore.getState().rename(scope, id, label)}
              />
            </div>
          );
        })}
      </div>
      <div ref={pagesRef} data-query-panes={panes.length} className="relative min-h-0 flex-1">
        {tabs
          .filter((tab) => opened.has(tab.id))
          .map((tab) => (
            <QueryTabPane
              key={tab.id}
              scope={scope}
              id={tab.id}
              active={tab.id === activeId}
              slot={panes.indexOf(tab.id)}
              slots={panes.length}
              onParamsWrite={onParamsWrite}
              onWorkedIn={workIn}
            >
              {children}
            </QueryTabPane>
          ))}
      </div>
    </div>
  );
}

const HOST_CLASS =
  'flex h-[calc(100vh-var(--topbar-h)-var(--contextbar-h,0px))] min-h-0 flex-col overflow-hidden bg-[var(--page-canvas)]';

/** The least a half of a split page is given. A window with less than two of these is not split. */
const MIN_PANE_WIDTH = 360;
/** Under this a page folds what stands beside its results: a list of fields takes most of it. */
const NARROW_PANE_WIDTH = 900;
/** The gutter a half keeps towards the other: the two make the seam of a split page. */
const SEAM_GUTTER = '6px';

/**
 * The side gutters of what stands in a half of the page: the page's own on the
 * outside, a narrow one towards the other half, so the seam between two halves
 * is not as wide as two page gutters. A page that is not split keeps its own.
 */
function paneGutters(slot: number, slots: number): React.CSSProperties | undefined {
  if (slots < 2) return undefined;

  const gutters: Record<string, string> =
    slot === 0 ? { '--pane-gutter-end': SEAM_GUTTER } : { '--pane-gutter-start': SEAM_GUTTER };

  return gutters as React.CSSProperties;
}

function tabElementId(id: string): string {
  return `query-tab-${id}`;
}

function panelElementId(id: string): string {
  return `query-tab-panel-${id}`;
}

const QueryTabPane = React.memo(function QueryTabPane({
  scope,
  id,
  active,
  slot,
  slots,
  onParamsWrite,
  onWorkedIn,
  children,
}: {
  scope: string;
  id: string;
  active: boolean;
  /** Where the tab stands on the page, left to right; -1 while it is behind. */
  slot: number;
  /** How many tabs stand on the page. */
  slots: number;
  /** Whether the write about to reach the address bar replaces the history entry. */
  onParamsWrite: (replace: boolean) => void;
  /** The tab was touched while another one was in front. */
  onWorkedIn: (id: string) => void;
  children: React.ReactNode;
}) {
  const tab = useQueryTabsStore(
    useShallow((store) => {
      const found = store.scopes[scope]?.tabs.find((item) => item.id === id);
      return found
        ? { search: found.search, window: found.window, anchor: found.anchor }
        : null;
    }),
  );
  const timeStore = React.useMemo(
    () => (tab ? queryTabTimeStore(scope, { id, window: tab.window, anchor: tab.anchor }) : null),
    // The store is made once per tab; later ranges are already in it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [id, scope, tab !== null],
  );
  const memory = React.useRef(
    useQueryTabsStore.getState().scopes[scope]?.tabs.find((item) => item.id === id)?.memory ?? {},
  ).current;

  const search = tab?.search ?? '';
  const window = tab?.window;
  const params = React.useMemo(
    () => (window ? tabSearchParams(search, window) : new URLSearchParams(search)),
    [search, window],
  );

  const setParams = React.useCallback<SetURLSearchParams>(
    (nextInit, options) => {
      const current = useQueryTabsStore.getState().scopes[scope]?.tabs.find((item) => item.id === id);
      if (!current) return;
      const previous = tabSearchParams(current.search, current.window);
      const next = createSearchParams(
        typeof nextInit === 'function' ? nextInit(previous) : nextInit,
      );
      onParamsWrite(options?.replace ?? false);
      useQueryTabsStore.getState().setSearch(scope, id, ownSearch(next));
    },
    [id, onParamsWrite, scope],
  );

  const remember = React.useCallback(
    (key: string, value: unknown) => useQueryTabsStore.getState().remember(scope, id, key, value),
    [id, scope],
  );

  const report = React.useCallback(
    (status: Partial<QueryTabStatus>) => {
      if (status.running !== undefined) {
        useQueryTabsRuntime.getState().setRunning(queryTabKey(scope, id), status.running);
      }
      if (status.summary !== undefined) {
        useQueryTabsStore.getState().setSummary(scope, id, status.summary);
      }
    },
    [id, scope],
  );

  const value = React.useMemo<QueryTabScope>(
    () => ({ id, active, params, setParams, memory, remember, report }),
    [active, id, memory, params, remember, report, setParams],
  );

  const shown = slot >= 0;
  const split = shown && slots > 1;
  const paneRef = React.useRef<HTMLDivElement>(null);
  const [narrow, setNarrow] = React.useState(false);
  const present = tab !== null;
  React.useLayoutEffect(() => {
    const pane = paneRef.current;
    if (!present || !pane || !shown || typeof ResizeObserver === 'undefined') return;

    const measure = () => setNarrow(pane.clientWidth < NARROW_PANE_WIDTH);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(pane);
    return () => observer.disconnect();
  }, [present, shown]);
  const pane = React.useMemo(() => ({ split, narrow }), [narrow, split]);

  if (!tab || !timeStore) return null;

  const workedIn = shown && !active ? () => onWorkedIn(id) : undefined;

  return (
    <div
      ref={paneRef}
      role="tabpanel"
      id={panelElementId(id)}
      aria-labelledby={tabElementId(id)}
      aria-hidden={!shown}
      data-query-tab-panel={id}
      data-active={active}
      data-query-pane={shown ? slot : undefined}
      // Touching a tab that stands beside the one in front brings it to the front.
      onPointerDownCapture={workedIn}
      onFocusCapture={workedIn}
      // The tab of each half has that half. A tab behind keeps the whole page as its layout
      // (and so its scroll position and chart sizes); it is only not painted and not reachable.
      style={
        split
          ? { left: `${(slot / slots) * 100}%`, width: `${100 / slots}%`, ...paneGutters(slot, slots) }
          : undefined
      }
      className={cn('absolute min-h-0', split ? 'inset-y-0' : 'inset-0', !shown && 'invisible')}
    >
      <TimeScopeContext.Provider value={timeStore}>
        <QueryTabScopeContext.Provider value={value}>
          <QueryPaneContext.Provider value={pane}>{children}</QueryPaneContext.Provider>
        </QueryTabScopeContext.Provider>
      </TimeScopeContext.Provider>
    </div>
  );
});
