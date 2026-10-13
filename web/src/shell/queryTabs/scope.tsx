import * as React from 'react';
import { type SetURLSearchParams, useSearchParams } from 'react-router-dom';

/**
 * What a page rendered in a query tab is given in place of the app's own:
 * search params that belong to the tab, a place to keep state with it, and a
 * way to tell the tab strip what it is doing.
 *
 * The tab's time range is not here: it comes through `TimeScopeContext`, so
 * `useTimeStore` already means "this tab's time" anywhere inside a tab.
 */
export interface QueryTabScope {
  id: string;
  /** In front. Tabs behind stay mounted and keep their results. */
  active: boolean;
  params: URLSearchParams;
  setParams: SetURLSearchParams;
  /** The state the tab came back with. Read once, when the page mounts. */
  memory: Readonly<Record<string, unknown>>;
  remember: (key: string, value: unknown) => void;
  report: (status: Partial<QueryTabStatus>) => void;
}

export interface QueryTabStatus {
  /** A query of this tab is in flight. */
  running: boolean;
  /** What the tab is querying, in a few words; it names a tab nobody named. */
  summary: string;
}

export const QueryTabScopeContext = React.createContext<QueryTabScope | null>(null);

/** What a page is told about its share of a page that may be split between tabs. */
export interface QueryPane {
  /** Other tabs stand beside this one. */
  split: boolean;
  /** The share is too narrow for what a page usually puts beside its results. */
  narrow: boolean;
}

export const QueryPaneContext = React.createContext<QueryPane>({ split: false, narrow: false });

/**
 * The share of the page a query tab has. A page that puts a list of fields or
 * a detail beside its results folds them while its share is narrow, as it does
 * in a narrow window. Outside a tab, the page has the window to itself.
 */
export function useQueryPane(): QueryPane {
  return React.useContext(QueryPaneContext);
}

/**
 * Whether a panel that stands beside a page's results is folded.
 *
 * While the page's share is narrow the panel is folded, unless the reader
 * opens it there; when the share is wide again the panel is as the reader had
 * it before. `initial` is how the page opens it in a wide share.
 */
export function useFoldedWhileNarrow(
  initial: boolean | (() => boolean),
): [boolean, (folded: boolean) => void] {
  const { narrow } = useQueryPane();
  const [chosen, setChosen] = React.useState(initial);
  // What the reader asked for in the narrow share they are in; forgotten when the share changes.
  const [asked, setAsked] = React.useState<{ narrow: boolean; folded: boolean } | null>(null);

  const folded = narrow ? (asked?.narrow ? asked.folded : true) : chosen;
  const setFolded = React.useCallback(
    (next: boolean) => {
      if (narrow) setAsked({ narrow: true, folded: next });
      else {
        setAsked(null);
        setChosen(next);
      }
    },
    [narrow],
  );

  React.useEffect(() => {
    if (!narrow) setAsked(null);
  }, [narrow]);

  return [folded, setFolded];
}

/**
 * `useSearchParams` for a page that can live in a query tab: the params are the
 * tab's own, and writing them changes the address bar only while the tab is in
 * front. Outside a tab it is the address bar's params.
 */
export function useQueryTabSearchParams(): [URLSearchParams, SetURLSearchParams] {
  const address = useSearchParams();
  const scope = React.useContext(QueryTabScopeContext);
  return scope ? [scope.params, scope.setParams] : address;
}

/** Whether the calling page is the query tab in front (always, outside a tab). */
export function useQueryTabActive(): boolean {
  return React.useContext(QueryTabScopeContext)?.active ?? true;
}

// Long enough to skip the keystrokes of a query being typed, short enough that
// a reload a moment later still finds it.
const REMEMBER_DELAY_MS = 200;

/**
 * `useState` whose value is kept with the query tab: a reload, or coming back
 * to the page, starts from the value the tab last had instead of `initial`.
 * Outside a tab it is plain state.
 *
 * Use it for what makes a tab the query it is (the stream, the statement), not
 * for results or transient UI.
 */
export function useQueryTabState<T>(
  key: string,
  initial: T | (() => T),
): [T, React.Dispatch<React.SetStateAction<T>>] {
  const scope = React.useContext(QueryTabScopeContext);
  const [value, setValue] = React.useState<T>(() => {
    const start = typeof initial === 'function' ? (initial as () => T)() : initial;
    const saved = scope?.memory[key];
    // Storage is not typed: a value of another shape is not the one that was kept.
    return saved !== undefined && typeof saved === typeof start ? (saved as T) : start;
  });

  const remember = scope?.remember;
  const latest = React.useRef(value);
  latest.current = value;

  React.useEffect(() => {
    if (!remember) return;
    const handle = window.setTimeout(() => remember(key, value), REMEMBER_DELAY_MS);
    return () => window.clearTimeout(handle);
  }, [key, remember, value]);

  // A tab closed or a page left within the delay still keeps its last value.
  React.useEffect(() => {
    if (!remember) return;
    return () => remember(key, latest.current);
  }, [key, remember]);

  return [value, setValue];
}

/** Tells the tab strip what this page is querying and whether a query is running. */
export function useQueryTabStatus({ running, summary }: QueryTabStatus): void {
  const report = React.useContext(QueryTabScopeContext)?.report;

  React.useEffect(() => {
    report?.({ running, summary });
  }, [report, running, summary]);

  // A tab whose page is gone is not running anything; it keeps its name.
  React.useEffect(() => {
    if (!report) return;
    return () => report({ running: false });
  }, [report]);
}
