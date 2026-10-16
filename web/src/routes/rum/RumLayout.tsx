import { Settings } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Outlet, useLocation } from 'react-router-dom';

import { ProductState, type ProductStateProps } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { type Fact, Facts } from '@/shell/readout/Facts';
import {
  surfacePanelClass,
  surfacePageRootClass,
  SurfacePageBody,
  SurfacePageHeader,
} from '@/shell/SurfaceWorkbench';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { FittedSelect } from '@/shell/ui/fitted-select';

// What is monitored, then what real users did there.
const TABS: Array<{ suffix: string; key: string; group: string }> = [
  { suffix: '/overview', key: 'overview', group: 'monitored' },
  { suffix: '/applications', key: 'applications', group: 'monitored' },
  { suffix: '/sessions', key: 'sessions', group: 'users' },
  { suffix: '/pages', key: 'pages', group: 'users' },
  { suffix: '/errors', key: 'errors', group: 'users' },
  { suffix: '/performance/overview', key: 'performance', group: 'users' },
  { suffix: '/session-replay', key: 'session_replay', group: 'users' },
];

const PERFORMANCE_TABS: Array<{
  suffix: string;
  key: 'overview' | 'web_vitals' | 'errors' | 'apis';
}> = [
  { suffix: '/performance/overview', key: 'overview' },
  { suffix: '/performance/web-vitals', key: 'web_vitals' },
  { suffix: '/performance/errors', key: 'errors' },
  { suffix: '/performance/apis', key: 'apis' },
];

const SETTINGS_TABS = [
  { suffix: '/settings/sdk', key: 'sdk' },
  { suffix: '/settings/source-maps', key: 'source_maps' },
  { suffix: '/settings/sampling', key: 'sampling' },
  { suffix: '/settings/privacy', key: 'privacy' },
  { suffix: '/settings/session-replay', key: 'session_replay' },
] as const;

/** Where the country of a RUM event comes from: DB-IP's City Lite database. */
const DBIP_URL = 'https://db-ip.com';

export function RumLayout() {
  return <Outlet />;
}

export function PerformanceTabs() {
  const { t } = useTranslation('rum');
  const basePath = useRumBasePath();
  return (
    <div
      data-rum-subnavigation="performance"
      className="flex h-[40px] items-center px-[10px] [@media(pointer:coarse)]:h-[44px]"
    >
      <ViewTabs
        label={t('nav.performance')}
        items={PERFORMANCE_TABS.map((tab) => ({
          key: tab.key,
          to: `${basePath}${tab.suffix}`,
          label: t(`performance.${tab.key}`),
        }))}
      />
    </div>
  );
}

export function RumSettingsTabs() {
  const { t } = useTranslation('rum');
  const basePath = useRumBasePath();
  return (
    <div
      data-rum-subnavigation="settings"
      className="flex h-[40px] items-center px-[10px] [@media(pointer:coarse)]:h-[44px]"
    >
      <ViewTabs
        label={t('settings.title')}
        items={SETTINGS_TABS.map((tab) => ({
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
 * tabs say which one is open, and the sub-tabs of Performance and of the
 * settings say which of theirs.
 */
export function RumListPage({
  toolbar,
  lead,
  performance,
  settings,
  state,
  filterBar,
  scope,
  bodyClassName,
  children,
}: {
  toolbar?: React.ReactNode | undefined;
  /** What the open tab found, in a line over its content. */
  lead?: React.ReactNode | undefined;
  performance?: boolean | undefined;
  settings?: boolean | undefined;
  state?: ProductStateProps | null | undefined;
  filterBar?: React.ReactNode | undefined;
  /**
   * What the whole page is scoped to (application, environment, …), drawn as a
   * row of the navigation card: where the reader is and what they are looking
   * at are answered in one place.
   */
  scope?: React.ReactNode | undefined;
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
      <RumNavigation performance={performance} settings={settings} scope={scope} />
      <SurfacePageBody className={cn('space-y-[12px]', bodyClassName)}>
        {!state && lead}
        {filterBar && (
          // One row: every control names itself, so none has a caption over it.
          <div
            data-rum-filters
            className="flex flex-wrap items-center gap-2 rounded-md bg-[var(--functional-surface)] px-[12px] py-[8px] [box-shadow:var(--shadow-functional-surface)]"
          >
            {filterBar}
          </div>
        )}
        {state ? (
          <ProductState
            {...state}
            className={cn(
              'rounded-md border-0 bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
              state.className,
            )}
          />
        ) : children}
        {!state && !settings && <RumGeoCredit />}
      </SurfacePageBody>
    </div>
  );
}

export function RumDetailPage({
  title,
  subtitle,
  toolbar,
  state,
  bodyClassName,
  children,
}: {
  title: React.ReactNode;
  subtitle?: string | undefined;
  toolbar?: React.ReactNode | undefined;
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
        {state ? (
          <ProductState
            {...state}
            className={cn(
              'rounded-md border-0 bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
              state.className,
            )}
          />
        ) : children}
        {!state && <RumGeoCredit />}
      </SurfacePageBody>
    </div>
  );
}

/**
 * The credit DB-IP's licence asks for on every web page that shows results from
 * its database. The country of a RUM event comes from DB-IP City Lite, so a RUM
 * page that can show one ends with this line. Like the credit at the foot of a
 * public status page, it is the quietest thing on the page.
 */
function RumGeoCredit() {
  const { t } = useTranslation('rum');

  return (
    <p data-rum-geo-credit className="m-0 text-xs text-tx-2">
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

/** The widest a filter of a RUM page gets; under it, a filter is as wide as its longest choice. */
const FILTER_MAX_WIDTH = 220;

/**
 * A filter of a RUM page. It has no caption: its first choice is the "all" of
 * its dimension and names it ("All regions"), and a picked value is said with
 * what it is a value of ("Region DE").
 */
export function RumFilterSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: ReadonlyArray<{ value: string; label: string }>;
  onChange: (value: string) => void;
}) {
  const narrowed = value !== options[0]?.value;

  return (
    <FittedSelect
      data-rum-filter
      aria-label={label}
      value={value}
      onValueChange={onChange}
      options={options}
      maxWidth={FILTER_MAX_WIDTH}
      prefix={narrowed ? label : undefined}
      className="font-strong text-tx-1"
    />
  );
}

export type RumFact = Fact;

/** What a RUM tab counted, in one line over its list. */
export function RumFacts({ label, facts }: { label: string; facts: readonly RumFact[] }) {
  return <Facts surface data-rum-facts="" label={label} facts={facts} />;
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
  performance,
  settings,
  scope,
}: {
  performance?: boolean | undefined;
  settings?: boolean | undefined;
  scope?: React.ReactNode | undefined;
}) {
  const { t } = useTranslation('rum');
  const basePath = useRumBasePath();
  const { pathname } = useLocation();
  return (
    <ModuleTabs
      label={t('title')}
      dataAttributes={{ 'data-rum-navigation': 'surface' }}
      items={TABS.map((tab) => ({
        key: tab.key,
        to: `${basePath}${tab.suffix}`,
        group: tab.group,
        label: t(`nav.${tab.key}`),
        // Performance is a section of four pages: all of them keep its tab lit.
        ...(tab.key === 'performance'
          ? { active: pathname.startsWith(`${basePath}/performance/`) }
          : {}),
      }))}
      trailing={[
        {
          key: 'settings',
          to: `${basePath}/settings/sdk`,
          icon: Settings,
          label: t('settings.title'),
          active: pathname.startsWith(`${basePath}/settings/`),
        },
      ]}
    >
      {(performance || settings) && (
        <div className="border-t border-bd-0">
          {settings ? <RumSettingsTabs /> : <PerformanceTabs />}
        </div>
      )}
      {scope && (
        // Starts where the first tab's label does, so the two rows share a left edge.
        <div
          data-rum-scope
          className="flex flex-wrap items-center gap-2 border-t border-bd-0 px-[20px] py-[8px]"
        >
          {scope}
        </div>
      )}
    </ModuleTabs>
  );
}

export function useRumBasePath(): '/rum' {
  return '/rum';
}
