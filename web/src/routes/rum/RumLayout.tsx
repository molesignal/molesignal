import { Settings } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Outlet, useLocation } from 'react-router-dom';

import { useOpenablePages } from '@/product/moduleAccess';
import { ProductState, type ProductStateProps } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import {
  surfacePanelClass,
  surfacePageRootClass,
  SurfacePageBody,
  SurfacePageHeader,
} from '@/shell/SurfaceWorkbench';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';
import { ViewTabs } from '@/shell/tabs/ViewTabs';

// How they did, then what real users did, what went wrong, how slow it was, what frustrated them, then what sends it.
const TABS: Array<{ suffix: string; key: string }> = [
  { suffix: '/overview', key: 'overview' },
  { suffix: '/explorer', key: 'explorer' },
  { suffix: '/playlists', key: 'playlists' },
  { suffix: '/errors', key: 'errors' },
  { suffix: '/performance', key: 'performance' },
  { suffix: '/frustration', key: 'frustration' },
  { suffix: '/analytics', key: 'analytics' },
  { suffix: '/feature-flags', key: 'feature_flags' },
  { suffix: '/applications', key: 'applications' },
];

const SETTINGS_TABS = [
  { suffix: '/settings/sdk', key: 'sdk' },
  { suffix: '/settings/source-maps', key: 'source_maps' },
  { suffix: '/settings/sampling', key: 'sampling' },
  { suffix: '/settings/privacy', key: 'privacy' },
  { suffix: '/settings/session-replay', key: 'session_replay' },
] as const;

/** The module's pages, in the order it opens on the first the reader may see. */
export const RUM_PAGES = TABS.map((tab) => `/rum${tab.suffix}`) as [string, ...string[]];

/** Where the country of a RUM event comes from: DB-IP's City Lite database. */
const DBIP_URL = 'https://db-ip.com';

export function RumLayout() {
  return <Outlet />;
}

export function RumSettingsTabs() {
  const { t } = useTranslation('rum');
  const basePath = useRumBasePath();
  const tabs = useOpenablePages(SETTINGS_TABS, (tab) => `${basePath}${tab.suffix}`);
  return (
    <div
      data-rum-subnavigation="settings"
      className="flex h-[40px] items-center px-[10px] [@media(pointer:coarse)]:h-[44px]"
    >
      <ViewTabs
        label={t('settings.title')}
        items={tabs.map((tab) => ({
          key: tab.key,
          to: `${basePath}${tab.suffix}`,
          label: t(`settings.nav.${tab.key}`),
        }))}
      />
    </div>
  );
}

/**
 * The frame every tab of RUM stands in.
 *
 * The module has one name, so the title does not change from tab to tab: the
 * tabs say which one is open, and the sub-tabs of the settings say which of theirs.
 */
export function RumListPage({
  toolbar,
  settings,
  navigationEnd,
  navigationScope,
  navigationViews,
  state,
  bodyClassName,
  children,
}: {
  toolbar?: React.ReactNode | undefined;
  settings?: boolean | undefined;
  /** What the whole module is looking at, at the far end of the tabs' row: its time range. */
  navigationEnd?: React.ReactNode | undefined;
  /** What the page is scoped to, as the second row of the navigation card. */
  navigationScope?: React.ReactNode | undefined;
  /** The views of a tab that has several, as the last row of the navigation card. */
  navigationViews?: React.ReactNode | undefined;
  state?: ProductStateProps | null | undefined;
  bodyClassName?: string | undefined;
  children?: React.ReactNode | undefined;
}) {
  const { t } = useTranslation('rum');

  return (
    <div
      data-surface-workbench-page="rum"
      className={surfacePageRootClass}
    >
      <SurfacePageHeader
        title={t('title')}
        subtitle={t('subtitle')}
        toolbar={toolbar}
        breadcrumbs={null}
        backTo={null}
      />
      <RumNavigation settings={settings} end={navigationEnd} scope={navigationScope} views={navigationViews} />
      <SurfacePageBody className={cn('space-y-[12px]', bodyClassName)}>
        {state ? (
          <ProductState
            {...state}
            className={cn(
              'rounded-md border-0 bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
              state.className,
            )}
          />
        ) : children}
      </SurfacePageBody>
    </div>
  );
}

/**
 * The frame of the explorer: the module's tabs over a page that fills what is left, as the other
 * explorers do. The explorer carries the module's name in its own bar, so there is no title row.
 */
export function RumExplorerFrame({ children }: { children: React.ReactNode }) {
  return (
    <div data-surface-workbench-page="rum" className={cn(surfacePageRootClass, 'flex h-full flex-col')}>
      <div className="mt-[12px] shrink-0">
        <RumNavigation />
      </div>
      {/* The query tabs fill what the navigation leaves, not the whole window. */}
      <div className="min-h-0 flex-1 [--query-tabs-height:100%]">{children}</div>
    </div>
  );
}

export function RumDetailPage({
  title,
  subtitle,
  toolbar,
  lead,
  credit = true,
  state,
  bodyClassName,
  children,
}: {
  title: React.ReactNode;
  subtitle?: string | undefined;
  toolbar?: React.ReactNode | undefined;
  /** What stands above the body, and stays when the page has nothing else to show. */
  lead?: React.ReactNode | undefined;
  /** The page shows places looked up from addresses, and says where they come from; true unless told otherwise. */
  credit?: boolean | undefined;
  state?: ProductStateProps | null | undefined;
  bodyClassName?: string | undefined;
  children?: React.ReactNode | undefined;
}) {
  return (
    <div
      data-surface-workbench-page="rum"
      className={surfacePageRootClass}
    >
      <SurfacePageHeader
        title={title}
        subtitle={subtitle}
        toolbar={toolbar}
      />
      <RumNavigation />
      <SurfacePageBody className={cn('space-y-[12px]', bodyClassName)}>
        {lead}
        {state ? (
          <ProductState
            {...state}
            className={cn(
              'rounded-md border-0 bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
              state.className,
            )}
          />
        ) : children}
        {!state && credit && <RumGeoCredit />}
      </SurfacePageBody>
    </div>
  );
}

/**
 * The credit DB-IP's licence asks for on every web page that shows results from
 * its database. The country of a RUM event comes from DB-IP City Lite, so a RUM
 * page that shows one (the explorer, a session) ends with this line. Like the
 * credit at the foot of a public status page, it is the quietest thing on the page.
 */
export function RumGeoCredit({ className }: { className?: string }) {
  const { t } = useTranslation('rum');

  return (
    <p data-rum-geo-credit className={cn('m-0 text-xs text-tx-2', className)}>
      <a
        href={DBIP_URL}
        target="_blank"
        rel="noopener noreferrer"
        className="rounded-sm underline-offset-4 transition-colors hover:text-tx-0 hover:underline focus-visible:bg-bg-2 focus-visible:text-tx-0"
      >
        {t('geo_credit')}
      </a>
    </p>
  );
}

export function RumSectionHeader({
  title,
  description,
  action,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    // The action stands beside the title, at its middle: set on the bottom of a two-line title
    // it sat against the list under it and far from the card's edge.
    <div data-rum-section-head className="flex min-w-0 flex-wrap items-center justify-between gap-3">
      <div className="min-w-0">
        <h2 className="m-0 type-section-title font-sans font-display text-tx-0">{title}</h2>
        {description && <p className="mb-0 mt-1 text-xs text-tx-3">{description}</p>}
      </div>
      {action}
    </div>
  );
}

export function RumSurface({
  className,
  ...props
}: React.ComponentProps<'section'>) {
  return (
    <section
      {...props}
      className={cn(surfacePanelClass, className)}
    />
  );
}

function RumNavigation({
  settings,
  end,
  scope,
  views,
}: {
  settings?: boolean | undefined;
  end?: React.ReactNode | undefined;
  scope?: React.ReactNode | undefined;
  views?: React.ReactNode | undefined;
}) {
  const { t } = useTranslation('rum');
  const basePath = useRumBasePath();
  const { pathname } = useLocation();
  const tabs = useOpenablePages(TABS, (tab) => `${basePath}${tab.suffix}`);
  const [settingsTab] = useOpenablePages(SETTINGS_TABS, (tab) => `${basePath}${tab.suffix}`);
  return (
    <ModuleTabs
      label={t('title')}
      dataAttributes={{ 'data-rum-navigation': 'surface' }}
      end={end}
      items={tabs.map((tab) => ({
        key: tab.key,
        to: `${basePath}${tab.suffix}`,
        label: t(`nav.${tab.key}`),
        // A session is opened from the explorer: its page keeps that tab lit.
        ...(tab.key === 'explorer' ? { active: pathname.startsWith(`${basePath}/sessions/`) || pathname === `${basePath}/explorer` } : {}),
      }))}
      trailing={
        settingsTab
          ? [
              {
                key: 'settings',
                to: `${basePath}${settingsTab.suffix}`,
                icon: Settings,
                label: t('settings.title'),
                active: pathname.startsWith(`${basePath}/settings/`),
              },
            ]
          : []
      }
    >
      {settings && (
        <div className="border-t border-bd-0">
          <RumSettingsTabs />
        </div>
      )}
      {scope && (
        // Starts where the first tab's icon does, so the two rows share a left edge.
        <div data-rum-scope-row="" className="flex flex-wrap items-center gap-2 border-t border-bd-0 px-[20px] py-[8px]">
          {scope}
        </div>
      )}
      {views && <div className="border-t border-bd-0">{views}</div>}
    </ModuleTabs>
  );
}

export function useRumBasePath(): '/rum' {
  return '/rum';
}
