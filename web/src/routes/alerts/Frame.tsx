import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';

import { AlertsSubNav } from './Layout';

/**
 * The frame every tab of the Alerts module stands in.
 *
 * The module has one name. Which tab is open is said by the tabs, so the title
 * above them does not change from tab to tab, and the action in the header is
 * the one that belongs to the tab being read.
 */
export function AlertsPage({
  action,
  tabsEnd,
  lead,
  listHead,
  state,
  contentSurface,
  children,
}: {
  /** The tab's own action: the thing that can be added to what it lists. */
  action?: React.ReactNode;
  /** What the tab is looking at, at the far end of the tabs: a time range. */
  tabsEnd?: React.ReactNode;
  lead?: React.ReactNode;
  listHead?: React.ReactNode;
  state?: ProductStateProps | null;
  contentSurface?: boolean;
  children?: React.ReactNode;
}) {
  const { t } = useTranslation('alerts');

  return (
    <ListPage
      appearance="surface"
      title={t('title')}
      subtitle={t('subtitle')}
      subnav={<AlertsSubNav end={tabsEnd} />}
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
