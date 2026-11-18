import * as React from 'react';

import { useUiPreference } from '@/shell/uiPreference/useUiPreference';

/** The main menu's preference, kept per user on the server (`navigation.menu`). */
export const MENU_PREFERENCE_SCOPE = 'navigation.menu';
const MENU_PREFERENCE_KEY = 'menu';
/** The most areas remembered; the menu shows the latest few of them. */
const MAX_RECENT = 8;

export interface MenuPreference {
  /** Navigation entries by id, the latest first, each once. */
  recent: string[];
  /** Whether an entry opens in a new browser tab rather than in place. */
  newTab: boolean;
}

const NOTHING_KEPT: MenuPreference = { recent: [], newTab: false };

export function parseMenuPreference(raw: unknown): MenuPreference | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null;

  const value = raw as Record<string, unknown>;
  const recent = Array.isArray(value.recent)
    ? value.recent.filter((id): id is string => typeof id === 'string')
    : [];

  return { recent: recent.slice(0, MAX_RECENT), newTab: value.newTab === true };
}

/** `recent` with `id` at its head, once, and no longer than the server keeps. */
export function withVisit(recent: readonly string[], id: string): string[] {
  return [id, ...recent.filter((seen) => seen !== id)].slice(0, MAX_RECENT);
}

/**
 * The areas the user opened lately and whether the menu opens its entries in a new tab. Nothing
 * is written before the server has said what it keeps: an early visit would otherwise replace
 * the list another browser left there.
 */
export function useMenuPreference() {
  const kept = useUiPreference(MENU_PREFERENCE_SCOPE, MENU_PREFERENCE_KEY, parseMenuPreference);
  const value = kept.value ?? NOTHING_KEPT;
  const latest = React.useRef(value);
  latest.current = value;
  const { save, settled } = kept;

  const visit = React.useCallback(
    (id: string) => {
      if (!settled || latest.current.recent[0] === id) return;

      save({ ...latest.current, recent: withVisit(latest.current.recent, id) });
    },
    [save, settled],
  );
  const setNewTab = React.useCallback(
    (newTab: boolean) => save({ ...latest.current, newTab }),
    [save],
  );

  return { recent: value.recent, newTab: value.newTab, settled, visit, setNewTab };
}
