import { Settings } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { NavLink, Outlet, useLocation } from 'react-router-dom';

import { KpiStrip, type KpiStripItem } from '@/admin';
import { ProductState, type ProductStateProps } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { PageBody, PageHeader } from '@/shell/PageHeader';

const TABS: Array<{ suffix: string; key: string }> = [
  { suffix: '/overview', key: 'overview' },
  { suffix: '/applications', key: 'applications' },
  { suffix: '/sessions', key: 'sessions' },
  { suffix: '/pages', key: 'pages' },
  { suffix: '/errors', key: 'errors' },
  { suffix: '/performance/overview', key: 'performance' },
  { suffix: '/session-replay', key: 'session_replay' },
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

export function RumLayout() {
  return <Outlet />;
}

export function RumTabs() {
  const { t } = useTranslation('rum');
  const basePath = useRumBasePath();
  const location = useLocation();
  return (
    <nav
      aria-label={t('title')}
      className="flex min-h-11 items-end gap-6 overflow-x-auto"
    >
      {TABS.map((tab) => {
        const sectionActive =
          tab.key === 'performance' &&
          location.pathname.startsWith(`${basePath}/performance/`);
        return (
          <NavLink
            key={tab.suffix}
            to={`${basePath}${tab.suffix}`}
            className={({ isActive }) =>
              cn(
                'relative inline-flex h-11 shrink-0 items-center border-b-2 border-transparent px-0.5 font-sans text-sm font-strong text-tx-2 outline-none transition-colors duration-fast hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0',
                (isActive || sectionActive) && 'border-indigo text-tx-0',
              )
            }
          >
            {t(`nav.${tab.key}`)}
          </NavLink>
        );
      })}
      <NavLink
        to={`${basePath}/settings/sdk`}
        aria-label={t('settings.title')}
        className={({ isActive }) =>
          cn(
            'ml-auto inline-flex h-11 shrink-0 items-center gap-1.5 border-b-2 border-transparent px-0.5 text-xs font-strong text-tx-2 transition-colors hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0',
            (isActive || location.pathname.startsWith(`${basePath}/settings/`)) &&
              'border-indigo text-tx-0',
          )
        }
      >
        <Settings aria-hidden className="h-3.5 w-3.5" />
        {t('settings.title')}
      </NavLink>
    </nav>
  );
}

export function PerformanceTabs() {
  const { t } = useTranslation('rum');
  const basePath = useRumBasePath();
  return (
    <nav
      aria-label={t('nav.performance')}
      className="flex min-h-10 items-end gap-5 overflow-x-auto"
    >
      {PERFORMANCE_TABS.map((tab) => (
        <NavLink
          key={tab.suffix}
          to={`${basePath}${tab.suffix}`}
          className={({ isActive }) =>
            cn(
              'inline-flex h-10 shrink-0 items-center border-b-2 border-transparent px-0.5 font-sans text-xs font-strong text-tx-2 outline-none transition-colors duration-fast hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0',
              isActive && 'border-indigo text-indigo-soft',
            )
          }
        >
          {t(`performance.${tab.key}`)}
        </NavLink>
      ))}
    </nav>
  );
}

export function RumSettingsTabs() {
  const { t } = useTranslation('rum');
  const basePath = useRumBasePath();
  return (
    <nav
      aria-label={t('settings.title')}
      className="flex min-h-10 items-end gap-5 overflow-x-auto"
    >
      {SETTINGS_TABS.map((tab) => (
        <NavLink
          key={tab.suffix}
          to={`${basePath}${tab.suffix}`}
          className={({ isActive }) =>
            cn(
              'inline-flex h-10 shrink-0 items-center border-b-2 border-transparent px-0.5 text-xs font-strong text-tx-2 transition-colors hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0',
              isActive && 'border-indigo text-indigo-soft',
            )
          }
        >
          {t(`settings.nav.${tab.key}`)}
        </NavLink>
      ))}
    </nav>
  );
}

export function RumListPage({
  title,
  subtitle,
  toolbar,
  kpis,
  kpiClassName,
  performance,
  settings,
  state,
  filterBar,
  bodyClassName,
  children,
}: {
  title: React.ReactNode;
  subtitle?: string | undefined;
  toolbar?: React.ReactNode | undefined;
  kpis?: readonly KpiStripItem[] | undefined;
  kpiClassName?: string | undefined;
  performance?: boolean | undefined;
  settings?: boolean | undefined;
  state?: ProductStateProps | null | undefined;
  filterBar?: React.ReactNode | undefined;
  bodyClassName?: string | undefined;
  children?: React.ReactNode | undefined;
}) {
  return (
    <div className="min-h-0 bg-bg-0">
      <PageHeader
        title={<h1 className="m-0">{title}</h1>}
        subtitle={subtitle}
        toolbar={toolbar}
      />
      <RumNavigation performance={performance} settings={settings} />
      <PageBody className={cn('space-y-5', bodyClassName)}>
        {kpis && (
          <KpiStrip
            items={kpis}
            className={cn('xl:grid-cols-4', kpiClassName)}
          />
        )}
        {filterBar && (
          <div className="flex flex-wrap items-end gap-3 border-y border-bd-0 py-3">
            {filterBar}
          </div>
        )}
        {state ? <ProductState {...state} /> : children}
      </PageBody>
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
    <div className="min-h-0 bg-bg-0">
      <PageHeader
        title={<h1 className="m-0">{title}</h1>}
        subtitle={subtitle}
        toolbar={toolbar}
      />
      <RumNavigation />
      <PageBody className={cn('space-y-5', bodyClassName)}>
        {state ? <ProductState {...state} /> : children}
      </PageBody>
    </div>
  );
}

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
  return (
    <label className="grid gap-1">
      <span className="type-caption font-sans font-strong text-tx-3">{label}</span>
      <select
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="h-8 min-w-[132px] rounded-md border border-bd-1 bg-bg-1 px-2.5 font-sans text-xs font-strong text-tx-1 outline-none transition-colors focus-visible:bg-bg-2 focus-visible:text-tx-0"
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </label>
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
    <div className="flex min-w-0 flex-wrap items-end justify-between gap-3 border-b border-bd-0 pb-3">
      <div className="min-w-0">
        <h2 className="m-0 type-section-title font-sans font-display text-tx-0">{title}</h2>
        {description && <p className="mb-0 mt-1 text-xs text-tx-3">{description}</p>}
      </div>
      {action}
    </div>
  );
}

function RumNavigation({
  performance,
  settings,
}: {
  performance?: boolean | undefined;
  settings?: boolean | undefined;
}) {
  return (
    <div className="border-b border-bd-0 bg-bg-1 px-6">
      <RumTabs />
      {(performance || settings) && (
        <div className="border-t border-bd-0">
          {settings ? <RumSettingsTabs /> : <PerformanceTabs />}
        </div>
      )}
    </div>
  );
}

export function useRumBasePath(): '/rum' {
  return '/rum';
}
