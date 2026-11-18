import * as React from 'react';

import { accessibleProductNavigation, useProductAccess } from '@/product/access';
import {
  isProductRouteActive,
  PRODUCT_NAV_GROUP_META,
  PRODUCT_NAV_ITEMS,
  type ProductNavGroup,
  type ProductRouteMeta,
} from '@/product/ia';

/**
 * The menu's columns, left to right, by the groups of the navigation. The two groups of one
 * entry each share a column, so no column stands nearly empty.
 */
export const MENU_COLUMNS: readonly (readonly ProductNavGroup[])[] = [
  ['observe'],
  ['reliability'],
  ['data'],
  ['ai_ops', 'collaboration'],
  ['admin'],
];

export interface MenuGroup {
  group: ProductNavGroup;
  labelKey: string;
  entries: ProductRouteMeta[];
}

/**
 * The entries of the main menu the user may open: Home on its own, the others by group in their
 * columns, and every one by id for the recent list. A group with nothing open to the user is left
 * out, and so is a column left empty.
 */
export function useMenuEntries() {
  const access = useProductAccess();

  return React.useMemo(() => {
    const home = accessibleProductNavigation(access, 'home');
    const columns = MENU_COLUMNS.map((groups) =>
      groups
        .map((group): MenuGroup => ({
          group,
          labelKey: PRODUCT_NAV_GROUP_META[group].labelKey,
          entries: accessibleProductNavigation(access, group),
        }))
        .filter((group) => group.entries.length > 0),
    ).filter((column) => column.length > 0);
    const byId = new Map(
      [...home, ...columns.flat().flatMap((group) => group.entries)].map((entry) => [entry.id, entry]),
    );

    return { home, columns, byId };
  }, [access]);
}

/**
 * The navigation entry the page belongs to: the one whose area is the narrowest that holds it,
 * so a page under Settings counts as Settings and not as Home.
 */
export function activeNavigationEntry(pathname: string): ProductRouteMeta | undefined {
  return [...PRODUCT_NAV_ITEMS]
    .filter((entry) => isProductRouteActive(entry, pathname))
    .sort((a, b) => (b.activePrefix ?? b.path).length - (a.activePrefix ?? a.path).length)[0];
}
