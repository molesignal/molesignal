import * as React from 'react';
import { useLocation, useNavigate } from 'react-router-dom';

import { composeSearch, ownSearch, type QueryTab } from './model';
import { queryTabTimeStore, useQueryTabsStore } from './store';

/** Synchronize the foreground tab and the router, including in-flight navigations. */
export function useQueryTabAddress(
  scope: string,
  activeId: string | undefined,
  activeSearch: string | undefined,
  frontTab: () => QueryTab | undefined,
) {
  const location = useLocation();
  const navigate = useNavigate();
  const mirror = React.useRef<{
    /** The tab and params the address bar was last reconciled with. */
    seen: string | null;
    /** What was asked of the router and has not shown up yet. */
    pending: string | null;
    askedAt: string | null;
    replace: boolean;
  }>({ seen: null, pending: null, askedAt: null, replace: true });

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

    // Tabs can have identical queries and different time ranges. Switching between
    // them must publish the time too, before the shell reads the old URL back.
    const switchedTabs = book.seen !== null && !book.seen.startsWith(`${activeId}\n`);
    // The tab can return to the displayed query before the previous navigation commits.
    // Supersede that write even when the router still shows the desired address.
    if (switchedTabs || (book.pending !== null && book.pending !== wanted)) {
      book.seen = wanted;
      ask();
      return;
    }
    if (shown === activeSearch) {
      book.pending = null;
      book.seen = wanted;
      return;
    }
    if (book.pending !== null) {
      // Asked and not there: still on its way, or a write that started before
      // it landed on top. Ask again once the address has moved without us.
      if (book.askedAt !== location.key) ask();
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

  return React.useCallback((replace: boolean) => {
    mirror.current.replace = replace;
  }, []);
}
