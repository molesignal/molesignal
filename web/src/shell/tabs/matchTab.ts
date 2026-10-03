import type { To } from 'react-router-dom';

export interface RouteTab {
  key: string;
  to?: To | undefined;
  /** Match only the exact path instead of the path and everything below it. */
  end?: boolean | undefined;
  /** Overrides the route match, for a tab that owns several routes. */
  active?: boolean | undefined;
}

function pathOf(to: To): string {
  const path = typeof to === 'string' ? to : (to.pathname ?? '');
  return (path.split(/[?#]/)[0] ?? '').replace(/\/+$/, '') || '/';
}

function matches(pathname: string, tab: RouteTab): boolean {
  if (!tab.to) return false;
  const target = pathOf(tab.to);
  const current = pathname.replace(/\/+$/, '') || '/';
  if (current === target) return true;
  return !tab.end && target !== '/' && current.startsWith(`${target}/`);
}

/**
 * The one tab to mark as current. A tab that says whether it is active
 * decides for itself; otherwise the tab with the most specific matching path
 * wins, so `/checks` and `/checks/new` never both light up.
 */
export function pickActiveTab(
  tabs: readonly RouteTab[],
  pathname: string,
): string | undefined {
  const explicit = tabs.find((tab) => tab.active === true);
  if (explicit) return explicit.key;
  let best: { key: string; length: number } | undefined;
  for (const tab of tabs) {
    if (tab.active !== undefined || !tab.to || !matches(pathname, tab)) continue;
    const length = pathOf(tab.to).length;
    if (!best || length > best.length) best = { key: tab.key, length };
  }
  return best?.key;
}
