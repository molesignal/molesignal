import {
  type Anchor,
  isDefaultTimeWindow,
  type TimeWindow,
  writeTimeParams,
} from '@/stores/useTimeStore';

/** The explorers that keep several queries open side by side. */
export type QueryTabsKind = 'logs' | 'metrics' | 'traces';

/** Enough for a real investigation, few enough that every tab keeps a readable name. */
export const MAX_QUERY_TABS = 10;

/** A page is split in two halves at most: a third of a page shows too little of a query. */
export const MAX_QUERY_PANES = 2;

export interface QueryTabTime {
  window: TimeWindow;
  anchor: Anchor | null;
}

/**
 * One query of an explorer: its own search params, its own time range and
 * whatever state the page asked to keep with it. Results are not here; they
 * live in the mounted page and are fetched again after a reload.
 */
export interface QueryTab extends QueryTabTime {
  id: string;
  /** Names a tab nobody named: "Query 3". Never reused within a set of tabs. */
  ordinal: number;
  /** A name the user gave the tab. */
  label?: string;
  /** What the tab is querying, as its page describes it. */
  summary?: string;
  /** The page's own search params, normalized; the keys the shell's stores write are not in it. */
  search: string;
  /** State the page keeps with the tab, by key. */
  memory: Record<string, unknown>;
  /** The tab has moved on from the link it was opened with. */
  touched: boolean;
  /** When the tab was last active. The least recent one makes room at the limit. */
  usedAt: number;
  /** The half of a split page that lists the tab: the right one. A tab without it is listed on the left. */
  side?: 1;
}

export interface QueryTabsState {
  tabs: QueryTab[];
  activeId: string;
  nextOrdinal: number;
  /**
   * The tab shown in each half of a split page, left then right. Each half
   * lists its own tabs (`QueryTab.side`), and the tab in front is one of the
   * two. Absent while one tab has the page to itself.
   */
  panes?: string[];
}

type TabsShown = Pick<QueryTabsState, 'tabs' | 'activeId' | 'panes'>;

const sideOf = (tab: Pick<QueryTab, 'side'>): 0 | 1 => (tab.side === 1 ? 1 : 0);

/**
 * The tabs on the page: the one shown in each half of a split page, left then
 * right, or the one in front alone. What is saved is read through here, so a
 * split that names a closed tab, a tab of the other half, or leaves out the
 * one in front is never shown as it is.
 */
export function queryTabPanes(state: TabsShown): string[] {
  const panes = state.panes;
  if (panes?.length !== MAX_QUERY_PANES || !panes.includes(state.activeId)) return [state.activeId];

  const [left, right] = panes.map((id) => state.tabs.find((tab) => tab.id === id));
  const split = left && right && left.id !== right.id && sideOf(left) === 0 && sideOf(right) === 1;

  return split ? [...panes] : [state.activeId];
}

/** The tabs each half of the page lists, left then right; all of them in one list while the page is not split. */
export function queryTabSides(state: TabsShown): QueryTab[][] {
  if (queryTabPanes(state).length < 2) return [state.tabs];

  return [state.tabs.filter((tab) => sideOf(tab) === 0), state.tabs.filter((tab) => sideOf(tab) === 1)];
}

/** `state` with one list of tabs and one of them on the page. */
function unsplit(state: QueryTabsState): QueryTabsState {
  const { panes, ...rest } = state;
  const sided = state.tabs.some((tab) => tab.side !== undefined);
  if (!panes && !sided) return state;

  return {
    ...rest,
    tabs: sided ? state.tabs.map(({ side: _side, ...tab }) => tab) : state.tabs,
  };
}

/**
 * `state` as it can be shown. A split page keeps a tab in each half: the one
 * it showed where that is still there, else the one of that half used last,
 * and the tab in front always. A half that has no tab left gives the page
 * back to the other.
 */
function settled(state: QueryTabsState): QueryTabsState {
  const wanted = state.panes;
  if (wanted?.length !== MAX_QUERY_PANES) return unsplit(state);

  const front = state.tabs.find((tab) => tab.id === state.activeId);
  const shown: string[] = [];
  for (const side of [0, 1] as const) {
    const listed = state.tabs.filter((tab) => sideOf(tab) === side);
    if (listed.length === 0) return unsplit(state);

    const kept = front && sideOf(front) === side ? front : listed.find((tab) => tab.id === wanted[side]);
    shown.push((kept ?? listed.reduce((last, tab) => (tab.usedAt > last.usedAt ? tab : last))).id);
  }

  return shown[0] === wanted[0] && shown[1] === wanted[1] ? state : { ...state, panes: shown };
}

/** Brings a tab to the front. On a split page it is shown in the half that lists it; the other half stays as it is. */
function inFront(state: QueryTabsState, id: string): QueryTabsState {
  return settled({ ...state, activeId: id });
}

/**
 * Search params that belong to the shell's stores, not to a tab: the time range
 * and anchor (each tab has its own, written back from its time store) and the
 * investigation stack and pinned filters (shared by every tab and every page).
 */
const STORE_KEYS = ['time', 'anchor', 'stack', 'filters'] as const;
const SHARED_KEYS = ['stack', 'filters'] as const;

/** A tab's own part of a search string, in the one spelling tabs are compared by. */
export function ownSearch(search: string | URLSearchParams): string {
  const params = new URLSearchParams(search);
  for (const key of STORE_KEYS) params.delete(key);
  return params.toString();
}

/** What a tab's page reads as its search params: its own, plus its time range the way a URL carries it. */
export function tabSearchParams(search: string, window: TimeWindow): URLSearchParams {
  const params = new URLSearchParams(search);
  if (!isDefaultTimeWindow(window)) params.set('time', `${window.from}..${window.to}`);
  return params;
}

/** The address-bar search for a tab: its own params and time, and the shared keys the URL already has. */
export function composeSearch(
  own: string,
  locationSearch: string,
  time: QueryTabTime,
): string {
  const params = new URLSearchParams(own);
  const current = new URLSearchParams(locationSearch);
  for (const key of SHARED_KEYS) {
    const value = current.get(key);
    if (value !== null) params.set(key, value);
  }
  writeTimeParams(params, time.window, time.anchor);
  return params.toString();
}

function newTab(
  state: Pick<QueryTabsState, 'nextOrdinal'>,
  init: { search?: string; memory?: Record<string, unknown>; label?: string } & QueryTabTime,
  now: number,
): QueryTab {
  return {
    id: `q${state.nextOrdinal}-${now.toString(36)}`,
    ordinal: state.nextOrdinal,
    ...(init.label ? { label: init.label } : {}),
    search: init.search ?? '',
    window: init.window,
    anchor: init.anchor,
    memory: init.memory ?? {},
    touched: false,
    usedAt: now,
  };
}

function withTab(
  state: QueryTabsState,
  id: string,
  change: (tab: QueryTab) => QueryTab,
): QueryTabsState {
  let changed = false;
  const tabs = state.tabs.map((tab) => {
    if (tab.id !== id) return tab;
    const next = change(tab);
    if (next !== tab) changed = true;
    return next;
  });
  return changed ? { ...state, tabs } : state;
}

/** How the page was reached: a link inside the app, or a load of the address (reload, bookmark, history). */
export type QueryTabsEntry = 'link' | 'load';

export interface QueryTabsArrival {
  /** The own search params of the address the page was opened at. */
  own: string;
  /** The time range that address names, when it names one. */
  time: Partial<QueryTabTime>;
  /** The time range the app is on, for an address that names none. */
  currentTime: QueryTabTime;
  entry: QueryTabsEntry;
}

/**
 * The tabs to show when the page opens.
 *
 * A bare address brings back the tab that was active. An address that carries a
 * query opens it in a tab of its own, so following a link never overwrites a
 * query that is being worked on; a tab still showing exactly that link is reused
 * instead of opened twice. Either way the tab in front takes the time range the
 * app is on (or the one the address names), as every other page does.
 */
export function openQueryTabs(
  saved: QueryTabsState | undefined,
  arrival: QueryTabsArrival,
  now: number,
): QueryTabsState {
  const time: QueryTabTime = {
    window: arrival.time.window ?? arrival.currentTime.window,
    anchor: arrival.time.anchor === undefined ? arrival.currentTime.anchor : arrival.time.anchor,
  };

  if (!saved || saved.tabs.length === 0) {
    const tab = newTab({ nextOrdinal: 1 }, { search: arrival.own, ...time }, now);
    return { tabs: [tab], activeId: tab.id, nextOrdinal: 2 };
  }

  const front = (state: QueryTabsState, id: string): QueryTabsState =>
    inFront(withTab(state, id, (tab) => ({ ...tab, ...time, usedAt: now })), id);
  const active = saved.tabs.find((tab) => tab.id === saved.activeId) ?? saved.tabs[0]!;
  if (!arrival.own) return front(saved, active.id);

  // A reload lands on the tab it left, edits and all. A link is only "already
  // open" in a tab that still shows what the link asks for.
  const reusable = (tab: QueryTab) =>
    tab.search === arrival.own && (arrival.entry === 'load' || !tab.touched);
  const existing = reusable(active) ? active : saved.tabs.find(reusable);
  if (existing) return front(saved, existing.id);

  // A link opens in the half that is worked in.
  const tab = { ...newTab(saved, { search: arrival.own, ...time }, now), ...sideLike(active) };
  const kept = makeRoom(saved.tabs, saved.activeId);
  return inFront(
    { ...saved, tabs: [...kept, tab], nextOrdinal: saved.nextOrdinal + 1 },
    tab.id,
  );
}

/** The half a tab is listed in, for a tab that is to be listed beside it. */
function sideLike(tab: Pick<QueryTab, 'side'> | undefined): Pick<QueryTab, 'side'> {
  return tab?.side === 1 ? { side: 1 } : {};
}

/** Drops the tab used longest ago (never the one in front) when one more would pass the limit. */
function makeRoom(tabs: QueryTab[], activeId: string): QueryTab[] {
  if (tabs.length < MAX_QUERY_TABS) return tabs;
  const oldest = tabs
    .filter((tab) => tab.id !== activeId)
    .reduce<QueryTab | null>((least, tab) => (!least || tab.usedAt < least.usedAt ? tab : least), null);
  return oldest ? tabs.filter((tab) => tab.id !== oldest.id) : tabs;
}

/**
 * A new tab after `besideId` (the tab in front, when none is named), in the
 * same half of the page, on the same time range and with nothing else carried
 * over.
 */
export function addQueryTab(
  state: QueryTabsState,
  now: number,
  besideId: string = state.activeId,
): QueryTabsState {
  if (state.tabs.length >= MAX_QUERY_TABS) return state;
  const beside = state.tabs.find((tab) => tab.id === besideId)
    ?? state.tabs.find((tab) => tab.id === state.activeId);
  const tab = newTab(
    state,
    {
      window: beside?.window ?? state.tabs[0]!.window,
      anchor: beside?.anchor ?? null,
    },
    now,
  );
  return insertAfter(state, beside?.id ?? state.activeId, tab);
}

/** A copy of a tab, opened next to it: the same query, time range and kept state. */
export function duplicateQueryTab(
  state: QueryTabsState,
  id: string,
  now: number,
): QueryTabsState {
  const source = state.tabs.find((tab) => tab.id === id);
  if (!source || state.tabs.length >= MAX_QUERY_TABS) return state;
  const tab = newTab(
    state,
    {
      search: source.search,
      window: source.window,
      anchor: source.anchor,
      memory: { ...source.memory },
      ...(source.label ? { label: source.label } : {}),
    },
    now,
  );
  return insertAfter(state, id, { ...tab, ...(source.summary ? { summary: source.summary } : {}) });
}

/** Lists `tab` after the tab `id`, in the same half of the page, and brings it to the front. */
function insertAfter(state: QueryTabsState, id: string, tab: QueryTab): QueryTabsState {
  const index = state.tabs.findIndex((item) => item.id === id);
  const at = index < 0 ? state.tabs.length : index + 1;
  const { side: _side, ...unsided } = tab;
  const listed: QueryTab = { ...unsided, ...sideLike(state.tabs[index]) };
  return inFront(
    {
      ...state,
      tabs: [...state.tabs.slice(0, at), listed, ...state.tabs.slice(at)],
      nextOrdinal: state.nextOrdinal + 1,
    },
    listed.id,
  );
}

/** Brings a tab to the front. On a split page it is shown in the half that lists it. */
export function activateQueryTab(
  state: QueryTabsState,
  id: string,
  now: number,
): QueryTabsState {
  if (state.activeId === id || !state.tabs.some((tab) => tab.id === id)) return state;
  return inFront(withTab(state, id, (tab) => ({ ...tab, usedAt: now })), id);
}

/**
 * Splits the page in two halves, or gives it back to one tab.
 *
 * Splitting leaves every open tab on the left and opens a copy of the tab in
 * front on the right, in front: the same query, to be set against itself over
 * another range or with another filter. At the limit of tabs the tab used last
 * moves to the right instead. Each half then lists its own tabs. Unsplitting
 * puts them back in one list, with the tab in front on the page.
 */
export function setQueryPaneCount(state: QueryTabsState, count: number, now: number): QueryTabsState {
  const split = queryTabPanes(state).length > 1;
  if (count < 2) return split ? unsplit(state) : state;
  if (split) return state;

  const single = unsplit(state);
  const source = single.tabs.find((tab) => tab.id === single.activeId);
  if (!source) return state;

  if (single.tabs.length >= MAX_QUERY_TABS) {
    const other = single.tabs
      .filter((tab) => tab.id !== source.id)
      .reduce<QueryTab | null>((last, tab) => (!last || tab.usedAt > last.usedAt ? tab : last), null);
    if (!other) return state;

    return {
      ...single,
      tabs: [...single.tabs.filter((tab) => tab.id !== other.id), { ...other, side: 1, usedAt: now }],
      panes: [source.id, other.id],
      activeId: other.id,
    };
  }

  const copy: QueryTab = {
    ...newTab(
      single,
      {
        search: source.search,
        window: source.window,
        anchor: source.anchor,
        memory: { ...source.memory },
        ...(source.label ? { label: source.label } : {}),
      },
      now,
    ),
    ...(source.summary ? { summary: source.summary } : {}),
    side: 1,
  };

  return {
    ...single,
    tabs: [...single.tabs, copy],
    nextOrdinal: single.nextOrdinal + 1,
    panes: [source.id, copy.id],
    activeId: copy.id,
  };
}

/**
 * Moves a tab to the other half of a split page, where it is shown and in
 * front. The half it leaves shows the tab it used last; one that has no tab
 * left gives the page back to the other.
 */
export function moveQueryTabAcross(state: QueryTabsState, id: string, now: number): QueryTabsState {
  const tab = state.tabs.find((item) => item.id === id);
  if (!tab || queryTabPanes(state).length < 2) return state;

  const { side: _side, ...unsided } = tab;
  const moved: QueryTab = { ...unsided, ...(sideOf(tab) === 0 ? { side: 1 as const } : {}), usedAt: now };
  // Last in the list it joins, as a tab that is opened there would be.
  const tabs = [...state.tabs.filter((item) => item.id !== id), moved];

  return settled({ ...state, tabs, activeId: id });
}

/**
 * Closes a tab. The tab to its right comes forward (the one to its left when it
 * was last), and closing the only tab leaves a fresh one, so there is always a
 * query to work in.
 */
export function closeQueryTab(state: QueryTabsState, id: string, now: number): QueryTabsState {
  const index = state.tabs.findIndex((tab) => tab.id === id);
  if (index < 0) return state;
  const closing = state.tabs[index]!;

  if (state.tabs.length === 1) {
    const tab = newTab(state, { window: closing.window, anchor: closing.anchor }, now);
    return { tabs: [tab], activeId: tab.id, nextOrdinal: state.nextOrdinal + 1 };
  }

  const tabs = state.tabs.filter((tab) => tab.id !== id);
  if (state.activeId !== id) return settled({ ...state, tabs });

  // On a split page the neighbour is one of the same half; a half left without a tab gives
  // the page back to the other, whose tab comes to the front.
  const split = queryTabPanes(state).length > 1;
  const listed = split ? state.tabs.filter((tab) => sideOf(tab) === sideOf(closing)) : state.tabs;
  const at = listed.indexOf(closing);
  const neighbours = listed.filter((tab) => tab.id !== id);
  const next = neighbours[Math.min(at, neighbours.length - 1)]
    ?? tabs.find((tab) => state.panes?.includes(tab.id))
    ?? tabs[Math.min(index, tabs.length - 1)]!;

  return settled({
    ...state,
    tabs: tabs.map((tab) => (tab === next ? { ...tab, usedAt: now } : tab)),
    activeId: next.id,
  });
}

/** Closes every tab but one, and brings that one to the front. */
export function closeOtherQueryTabs(state: QueryTabsState, id: string, now: number): QueryTabsState {
  const keep = state.tabs.find((tab) => tab.id === id);
  if (!keep || state.tabs.length === 1) return state;
  return unsplit({ ...state, tabs: [{ ...keep, usedAt: now }], activeId: keep.id });
}

/** Names a tab; an empty name gives it back its own. */
export function renameQueryTab(state: QueryTabsState, id: string, label: string): QueryTabsState {
  const name = label.trim();
  return withTab(state, id, (tab) => {
    if ((tab.label ?? '') === name) return tab;
    const { label: _previous, ...rest } = tab;
    return name ? { ...rest, label: name } : rest;
  });
}

export function setQueryTabSummary(
  state: QueryTabsState,
  id: string,
  summary: string,
): QueryTabsState {
  const text = summary.trim();
  return withTab(state, id, (tab) => {
    if ((tab.summary ?? '') === text) return tab;
    const { summary: _previous, ...rest } = tab;
    return text ? { ...rest, summary: text } : rest;
  });
}

export function setQueryTabSearch(
  state: QueryTabsState,
  id: string,
  search: string,
): QueryTabsState {
  return withTab(state, id, (tab) =>
    tab.search === search ? tab : { ...tab, search, touched: true },
  );
}

export function setQueryTabTime(
  state: QueryTabsState,
  id: string,
  time: QueryTabTime,
): QueryTabsState {
  return withTab(state, id, (tab) =>
    tab.window === time.window && tab.anchor === time.anchor
      ? tab
      : { ...tab, window: time.window, anchor: time.anchor },
  );
}

/**
 * Keeps a piece of the page's state with the tab. The first value a key gets is
 * where the page started; a later, different one means the tab has moved on.
 */
export function rememberInQueryTab(
  state: QueryTabsState,
  id: string,
  key: string,
  value: unknown,
): QueryTabsState {
  return withTab(state, id, (tab) => {
    if (Object.is(tab.memory[key], value)) return tab;
    const known = Object.prototype.hasOwnProperty.call(tab.memory, key);
    return {
      ...tab,
      memory: { ...tab.memory, [key]: value },
      touched: tab.touched || known,
    };
  });
}

/** The name on a tab: the user's, else what it is querying, else none (the strip numbers it). */
export function queryTabName(tab: Pick<QueryTab, 'label' | 'summary'>): string | null {
  return tab.label || tab.summary || null;
}
