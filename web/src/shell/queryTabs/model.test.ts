import { describe, expect, it } from 'vitest';

import { DEFAULT_WINDOW, type TimeWindow } from '@/stores/useTimeStore';

import {
  activateQueryTab,
  addQueryTab,
  closeOtherQueryTabs,
  closeQueryTab,
  composeSearch,
  duplicateQueryTab,
  MAX_QUERY_TABS,
  openQueryTabs,
  ownSearch,
  type QueryTabsArrival,
  type QueryTabsState,
  queryTabName,
  rememberInQueryTab,
  renameQueryTab,
  setQueryTabSearch,
  setQueryTabSummary,
  setQueryTabTime,
  tabSearchParams,
} from './model';

const SIX_HOURS: TimeWindow = { from: 'now-6h', to: 'now', mode: 'relative' };
const DAY: TimeWindow = { from: 'now-24h', to: 'now', mode: 'relative' };

function arrival(overrides: Partial<QueryTabsArrival> = {}): QueryTabsArrival {
  return {
    own: '',
    time: {},
    currentTime: { window: DEFAULT_WINDOW, anchor: null },
    entry: 'link',
    ...overrides,
  };
}

/** A set of tabs opened one link after another, the way a visit builds them. */
function opened(...searches: string[]): QueryTabsState {
  let state: QueryTabsState | undefined;
  searches.forEach((own, index) => {
    state = openQueryTabs(state, arrival({ own }), 1_000 + index);
  });
  return state!;
}

describe('search params of a tab', () => {
  it('keeps the page own params and leaves out what the shell stores write', () => {
    expect(ownSearch('?q=error&time=now-6h..now&filters=%5B%5D&stack=abc&anchor=x&stream=app')).toBe(
      'q=error&stream=app',
    );
    // Explicit `from` / `to` belong to the link that carried them.
    expect(ownSearch('from=a&to=b')).toBe('from=a&to=b');
  });

  it('gives the page its time range the way a URL would carry it', () => {
    expect(tabSearchParams('q=error', DEFAULT_WINDOW).has('time')).toBe(false);
    expect(tabSearchParams('q=error', SIX_HOURS).get('time')).toBe('now-6h..now');
  });

  it('composes the address from the tab, its time and the shared keys already there', () => {
    const search = composeSearch('q=error', '?q=old&filters=%5B1%5D&stack=s&time=now-24h..now', {
      window: SIX_HOURS,
      anchor: { at: '2026-05-23T10:00:00Z' },
    });
    const params = new URLSearchParams(search);

    expect(params.get('q')).toBe('error');
    expect(params.get('filters')).toBe('[1]');
    expect(params.get('stack')).toBe('s');
    expect(params.get('time')).toBe('now-6h..now');
    expect(params.get('anchor')).toBe('2026-05-23T10:00:00Z');
    // The default range is not written, as everywhere else in the app.
    expect(composeSearch('q=error', '', { window: DEFAULT_WINDOW, anchor: null })).toBe('q=error');
  });
});

describe('openQueryTabs', () => {
  it('starts with one tab holding what the address asks for', () => {
    const state = openQueryTabs(
      undefined,
      arrival({ own: 'q=error', time: { window: SIX_HOURS } }),
      1_000,
    );

    expect(state.tabs).toHaveLength(1);
    expect(state.tabs[0]).toMatchObject({ search: 'q=error', window: SIX_HOURS, ordinal: 1 });
    expect(state.activeId).toBe(state.tabs[0]!.id);
  });

  it('brings back the tab that was in front on a bare address, on the time range the app is on', () => {
    const saved = opened('q=first', 'q=second');
    const front = saved.activeId;

    const state = openQueryTabs(
      saved,
      arrival({ currentTime: { window: SIX_HOURS, anchor: null } }),
      5_000,
    );

    expect(state.activeId).toBe(front);
    expect(state.tabs.find((tab) => tab.id === front)?.window).toEqual(SIX_HOURS);
    // The tab behind keeps the range it was left on.
    expect(state.tabs.find((tab) => tab.id !== front)?.window).toEqual(DEFAULT_WINDOW);
  });

  it('prefers the time range the address names over the one the app is on', () => {
    const state = openQueryTabs(
      opened('q=first'),
      arrival({ time: { window: DAY }, currentTime: { window: SIX_HOURS, anchor: null } }),
      5_000,
    );

    expect(state.tabs[0]?.window).toEqual(DAY);
  });

  it('opens a link in a tab of its own instead of overwriting the query in front', () => {
    const saved = opened('q=first');

    const state = openQueryTabs(saved, arrival({ own: 'q=from-link' }), 5_000);

    expect(state.tabs.map((tab) => tab.search)).toEqual(['q=first', 'q=from-link']);
    expect(state.activeId).toBe(state.tabs[1]!.id);
    expect(state.tabs[1]?.ordinal).toBe(2);
  });

  it('reuses a tab that still shows exactly what the link asks for', () => {
    const saved = opened('q=first', 'q=second');

    const state = openQueryTabs(saved, arrival({ own: 'q=first' }), 5_000);

    expect(state.tabs).toHaveLength(2);
    expect(state.activeId).toBe(saved.tabs[0]!.id);
  });

  it('does not hand a link a tab that has moved on from it', () => {
    const saved = opened('q=first');
    const moved = rememberInQueryTab(
      rememberInQueryTab(saved, saved.activeId, 'statement', 'a'),
      saved.activeId,
      'statement',
      'b',
    );

    const fromLink = openQueryTabs(moved, arrival({ own: 'q=first', entry: 'link' }), 5_000);
    expect(fromLink.tabs).toHaveLength(2);

    // A reload is not a link: it lands on the tab it left, edits and all.
    const fromReload = openQueryTabs(moved, arrival({ own: 'q=first', entry: 'load' }), 5_000);
    expect(fromReload.tabs).toHaveLength(1);
    expect(fromReload.tabs[0]?.memory).toEqual({ statement: 'b' });
  });

  it('makes room at the limit by dropping the tab used longest ago, never the one in front', () => {
    const full = opened(...Array.from({ length: MAX_QUERY_TABS }, (_, index) => `q=${index}`));
    const front = full.activeId;

    const state = openQueryTabs(full, arrival({ own: 'q=one-more' }), 9_000);

    expect(state.tabs).toHaveLength(MAX_QUERY_TABS);
    expect(state.tabs.map((tab) => tab.search)).not.toContain('q=0');
    expect(state.tabs.some((tab) => tab.id === front)).toBe(true);
    expect(state.tabs.at(-1)?.search).toBe('q=one-more');
  });
});

describe('working with tabs', () => {
  it('adds a tab after the one in front, on its time range and with nothing else', () => {
    const base = setQueryTabTime(opened('q=first', 'q=second'), opened('q=first').activeId, {
      window: SIX_HOURS,
      anchor: null,
    });
    const first = activateQueryTab(base, base.tabs[0]!.id, 2_000);
    const withRange = setQueryTabTime(first, first.activeId, { window: SIX_HOURS, anchor: null });

    const state = addQueryTab(withRange, 3_000);

    expect(state.tabs.map((tab) => tab.search)).toEqual(['q=first', '', 'q=second']);
    expect(state.activeId).toBe(state.tabs[1]!.id);
    expect(state.tabs[1]).toMatchObject({ window: SIX_HOURS, memory: {}, touched: false });
  });

  it('never numbers two tabs alike, even after one is closed', () => {
    let state = opened('q=a');
    state = addQueryTab(state, 2_000);
    state = closeQueryTab(state, state.activeId, 3_000);
    state = addQueryTab(state, 4_000);

    expect(state.tabs.map((tab) => tab.ordinal)).toEqual([1, 3]);
  });

  it('stops adding at the limit', () => {
    let state = opened('q=a');
    for (let index = 0; index < MAX_QUERY_TABS + 3; index += 1) state = addQueryTab(state, 2_000 + index);

    expect(state.tabs).toHaveLength(MAX_QUERY_TABS);
    expect(addQueryTab(state, 9_000)).toBe(state);
  });

  it('duplicates a tab next to it with its query, time range, name and kept state', () => {
    let state = opened('q=a', 'q=b');
    const source = state.tabs[0]!.id;
    state = rememberInQueryTab(state, source, 'stream', 'app');
    state = renameQueryTab(state, source, 'Errors');
    state = setQueryTabTime(state, source, { window: DAY, anchor: null });

    const next = duplicateQueryTab(state, source, 5_000);

    expect(next.tabs.map((tab) => tab.search)).toEqual(['q=a', 'q=a', 'q=b']);
    const copy = next.tabs[1]!;
    expect(next.activeId).toBe(copy.id);
    expect(copy.id).not.toBe(source);
    expect(copy).toMatchObject({ label: 'Errors', window: DAY, memory: { stream: 'app' } });
  });

  it('brings the tab to the right forward when the one in front closes, else the one to the left', () => {
    const state = opened('q=a', 'q=b', 'q=c');
    const [a, b, c] = state.tabs.map((tab) => tab.id) as [string, string, string];

    const middle = closeQueryTab(activateQueryTab(state, b, 2_000), b, 3_000);
    expect(middle.activeId).toBe(c);

    const last = closeQueryTab(state, c, 3_000);
    expect(last.activeId).toBe(b);

    // Closing a tab behind leaves the one in front where it is.
    expect(closeQueryTab(state, a, 3_000).activeId).toBe(c);
  });

  it('leaves a fresh tab when the only one is closed', () => {
    const state = setQueryTabTime(opened('q=a'), opened('q=a').activeId, { window: DAY, anchor: null });

    const next = closeQueryTab(state, state.activeId, 3_000);

    expect(next.tabs).toHaveLength(1);
    expect(next.tabs[0]).toMatchObject({ search: '', window: DAY, ordinal: 2 });
    expect(next.tabs[0]?.id).not.toBe(state.activeId);
  });

  it('closes every other tab and brings the kept one forward', () => {
    const state = opened('q=a', 'q=b', 'q=c');
    const keep = state.tabs[0]!.id;

    const next = closeOtherQueryTabs(state, keep, 3_000);

    expect(next.tabs.map((tab) => tab.id)).toEqual([keep]);
    expect(next.activeId).toBe(keep);
  });

  it('names a tab by what the user called it, else by what it queries', () => {
    let state = opened('q=a');
    const id = state.activeId;
    expect(queryTabName(state.tabs[0]!)).toBeNull();

    state = setQueryTabSummary(state, id, '  rate(http_requests_total[5m]) ');
    expect(queryTabName(state.tabs[0]!)).toBe('rate(http_requests_total[5m])');

    state = renameQueryTab(state, id, ' Checkout errors ');
    expect(queryTabName(state.tabs[0]!)).toBe('Checkout errors');

    // An empty name gives the tab back its own.
    state = renameQueryTab(state, id, '   ');
    expect(queryTabName(state.tabs[0]!)).toBe('rate(http_requests_total[5m])');
  });

  it('returns the same state when nothing changes, so nothing re-renders or re-saves', () => {
    const state = setQueryTabSummary(opened('q=a'), opened('q=a').activeId, 'errors');
    const id = state.activeId;

    expect(setQueryTabSummary(state, id, 'errors')).toBe(state);
    expect(setQueryTabSearch(state, id, 'q=a')).toBe(state);
    expect(renameQueryTab(state, id, '')).toBe(state);
    expect(activateQueryTab(state, id, 9_000)).toBe(state);
    expect(setQueryTabTime(state, id, { window: state.tabs[0]!.window, anchor: null })).toBe(state);
  });

  it('marks a tab as moved on when its params or kept state change, not when they are first set', () => {
    let state = opened('q=a');
    const id = state.activeId;

    state = rememberInQueryTab(state, id, 'stream', 'app');
    expect(state.tabs[0]?.touched).toBe(false);

    state = rememberInQueryTab(state, id, 'stream', 'web');
    expect(state.tabs[0]?.touched).toBe(true);

    const written = setQueryTabSearch(opened('q=a'), opened('q=a').activeId, 'q=b');
    expect(written.tabs[0]?.touched).toBe(true);
  });
});
