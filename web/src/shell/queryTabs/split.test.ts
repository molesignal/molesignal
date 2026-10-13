import { describe, expect, it } from 'vitest';

import { DEFAULT_WINDOW } from '@/stores/useTimeStore';

import {
  activateQueryTab,
  addQueryTab,
  closeOtherQueryTabs,
  closeQueryTab,
  duplicateQueryTab,
  MAX_QUERY_TABS,
  moveQueryTabAcross,
  openQueryTabs,
  type QueryTabsState,
  queryTabPanes,
  queryTabSides,
  setQueryPaneCount,
} from './model';

/** A set of tabs opened one link after another, the way a visit builds them. */
function opened(...searches: string[]): QueryTabsState {
  let state: QueryTabsState | undefined;
  searches.forEach((own, index) => {
    state = openQueryTabs(
      state,
      { own, time: {}, currentTime: { window: DEFAULT_WINDOW, anchor: null }, entry: 'link' },
      1_000 + index,
    );
  });

  return state!;
}

const ids = (state: QueryTabsState) => state.tabs.map((tab) => tab.id);
const sides = (state: QueryTabsState) => queryTabSides(state).map((listed) => listed.map((tab) => tab.search));
const shown = (state: QueryTabsState) =>
  queryTabPanes(state).map((id) => state.tabs.find((tab) => tab.id === id)!.search);
const front = (state: QueryTabsState) => state.tabs.find((tab) => tab.id === state.activeId)!;

describe('a page split in two halves', () => {
  it('is one tab with the page to itself until it is split', () => {
    const state = opened('q=a', 'q=b');

    expect(queryTabPanes(state)).toEqual([state.activeId]);
    expect(sides(state)).toEqual([['q=a', 'q=b']]);
  });

  it('keeps every tab on the left and opens a copy of the one in front on the right, in front', () => {
    const before = opened('q=a', 'q=b');
    const state = setQueryPaneCount(before, 2, 2_000);

    expect(sides(state)).toEqual([['q=a', 'q=b'], ['q=b']]);
    expect(shown(state)).toEqual(['q=b', 'q=b']);
    // The copy is a tab of its own, with the query, the range and the name of the one it copies.
    expect(state.tabs).toHaveLength(3);
    expect(front(state).side).toBe(1);
    expect(front(state).id).not.toBe(before.activeId);
    expect(front(state).window).toBe(front(before).window);
    // Splitting what is split changes nothing; three halves are not a thing.
    expect(setQueryPaneCount(state, 2, 2_001)).toBe(state);
    expect(setQueryPaneCount(state, 3, 2_001)).toBe(state);
  });

  it('moves the tab used last to the right when no more tabs can be opened', () => {
    const full = opened(...Array.from({ length: MAX_QUERY_TABS }, (_, index) => `q=${index}`));
    const state = setQueryPaneCount(full, 2, 5_000);

    expect(state.tabs).toHaveLength(MAX_QUERY_TABS);
    expect(shown(state)).toEqual(['q=9', 'q=8']);
    expect(sides(state)[1]).toEqual(['q=8']);
  });

  it('brings a tab to the front in the half that lists it, and leaves the other half as it is', () => {
    const state = setQueryPaneCount(opened('q=a', 'q=b'), 2, 2_000);
    const [a, b] = ids(state);

    const left = activateQueryTab(state, a!, 3_000);
    expect(shown(left)).toEqual(['q=a', 'q=b']);
    expect(left.activeId).toBe(a);

    const again = activateQueryTab(left, b!, 3_001);
    expect(shown(again)).toEqual(['q=b', 'q=b']);
    expect(again.activeId).toBe(b);
    // The right half still shows its own tab, which is not the one in front now.
    expect(again.panes?.[1]).toBe(state.activeId);
  });

  it('opens a new tab and a copy in the half they were asked for in', () => {
    const state = setQueryPaneCount(opened('q=a'), 2, 2_000);
    const leftShown = state.panes![0]!;

    const added = addQueryTab(state, 3_000, leftShown);
    expect(sides(added)).toEqual([['q=a', ''], ['q=a']]);
    expect(front(added).side).toBeUndefined();
    expect(shown(added)).toEqual(['', 'q=a']);

    const copied = duplicateQueryTab(state, state.activeId, 3_000);
    expect(sides(copied)).toEqual([['q=a'], ['q=a', 'q=a']]);
    expect(front(copied).side).toBe(1);

    // A link that is followed opens in the half that is worked in.
    const linked = openQueryTabs(
      state,
      { own: 'q=z', time: {}, currentTime: { window: DEFAULT_WINDOW, anchor: null }, entry: 'link' },
      4_000,
    );
    expect(sides(linked)).toEqual([['q=a'], ['q=a', 'q=z']]);
    expect(shown(linked)).toEqual(['q=a', 'q=z']);
  });

  it('shows the neighbour of a closed tab, in the same half', () => {
    const state = addQueryTab(setQueryPaneCount(opened('q=a', 'q=b'), 2, 2_000), 3_000);
    // Left: a, b. Right: the copy of b, and a new tab in front.
    expect(sides(state)).toEqual([['q=a', 'q=b'], ['q=b', '']]);

    const closed = closeQueryTab(state, state.activeId, 4_000);
    expect(sides(closed)).toEqual([['q=a', 'q=b'], ['q=b']]);
    expect(front(closed).side).toBe(1);
    expect(shown(closed)).toEqual(['q=b', 'q=b']);

    // A tab that is not in front leaves without anything coming forward.
    const other = closeQueryTab(state, ids(state)[0]!, 4_000);
    expect(other.activeId).toBe(state.activeId);
    expect(sides(other)).toEqual([['q=b'], ['q=b', '']]);
  });

  it('gives the page back to the other half when a half has no tab left', () => {
    const state = setQueryPaneCount(opened('q=a', 'q=b'), 2, 2_000);

    const closed = closeQueryTab(state, state.activeId, 3_000);
    expect(queryTabPanes(closed)).toHaveLength(1);
    expect(sides(closed)).toEqual([['q=a', 'q=b']]);
    expect(front(closed).search).toBe('q=b');
    expect(closed.tabs.every((tab) => tab.side === undefined)).toBe(true);

    const only = closeOtherQueryTabs(state, state.activeId, 3_000);
    expect(queryTabPanes(only)).toEqual([only.activeId]);
    expect(only.tabs).toHaveLength(1);
    expect(only.tabs[0]!.side).toBeUndefined();
  });

  it('puts the tabs back in one list when the split is taken off, with the tab in front on the page', () => {
    const state = setQueryPaneCount(opened('q=a', 'q=b'), 2, 2_000);
    const single = setQueryPaneCount(state, 1, 3_000);

    expect(single.panes).toBeUndefined();
    expect(single.activeId).toBe(state.activeId);
    expect(sides(single)).toEqual([['q=a', 'q=b', 'q=b']]);
    expect(setQueryPaneCount(single, 1, 3_001)).toBe(single);
  });

  it('moves a tab to the other half, where it is shown and in front', () => {
    const state = setQueryPaneCount(opened('q=a', 'q=b'), 2, 2_000);
    const [a] = ids(state);

    const moved = moveQueryTabAcross(state, a!, 3_000);
    expect(sides(moved)).toEqual([['q=b'], ['q=b', 'q=a']]);
    expect(moved.activeId).toBe(a);
    expect(shown(moved)).toEqual(['q=b', 'q=a']);

    // The last tab of a half takes the half with it.
    const merged = moveQueryTabAcross(moved, ids(moved)[0]!, 3_001);
    expect(queryTabPanes(merged)).toHaveLength(1);
    expect(merged.tabs).toHaveLength(3);

    // On a page that is not split there is nowhere to move to.
    const single = opened('q=a');
    expect(moveQueryTabAcross(single, single.activeId, 1)).toBe(single);
  });

  it('never shows a split that was saved in a shape it cannot have', () => {
    const state = opened('q=a', 'q=b');
    const [a, b] = ids(state);

    // Two tabs of the same half, a tab that is gone, three halves, the tab in front left out.
    for (const panes of [[a!, b!], [a!, 'gone'], [a!, b!, a!], [a!]]) {
      expect(queryTabPanes({ ...state, panes })).toEqual([state.activeId]);
    }
    // The next change puts it right.
    const repaired = activateQueryTab({ ...state, panes: [a!, b!] }, a!, 2_000);
    expect(repaired.panes).toBeUndefined();
  });
});
