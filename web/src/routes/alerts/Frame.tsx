import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { useAlertsSidebarStore } from '@/stores/sidebarCollapse';

import { AlertsNav } from './Layout';

/**
 * The frame every page of the Alerts module stands in.
 *
 * The module has one name. Which page is open is said by the menu of its pages
 * beside it, so the title above does not change from page to page, and the
 * actions in the header are the ones that belong to the page being read.
 */
export function AlertsPage({
  action,
  lead,
  listHead,
  state,
  contentSurface,
  children,
}: {
  /** The page's own actions: the thing that can be added to what it lists, what it is looking at. */
  action?: React.ReactNode;
  lead?: React.ReactNode;
  listHead?: React.ReactNode;
  state?: ProductStateProps | null;
  contentSurface?: boolean;
  children?: React.ReactNode;
}) {
  const { t } = useTranslation('alerts');
  // A wide page (a timetable, a table of many columns) is read with the menu folded away.
  const folded = useAlertsSidebarStore((state) => state.collapsed);
  const unfold = useAlertsSidebarStore((state) => state.toggle);

  return (
    <ListPage
      appearance="surface"
      title={t('title')}
      subtitle={t('subtitle')}
      sections={<AlertsNav />}
      sectionNavigation={{ collapsed: folded, onExpand: unfold, expandLabel: t('subnav.expand_navigation') }}
      toolbar={action}
      lead={lead}
      listHead={listHead}
      state={state}
      {...(contentSurface !== undefined ? { contentSurface } : {})}
    >
      {children}
    </ListPage>
  );
}
