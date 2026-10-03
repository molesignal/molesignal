import { Settings } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Outlet, useLocation } from 'react-router-dom';

import { KpiStrip, type KpiStripItem } from '@/admin';
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

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
    <div
      data-surface-workbench-page="rum"
      className={surfacePageRootClass}
    >
      <SurfacePageHeader
        title={title}
        subtitle={subtitle}
        toolbar={toolbar}
        breadcrumbs={null}
        backTo={null}
      />
      <RumNavigation performance={performance} settings={settings} />
      <SurfacePageBody className={cn('space-y-[12px]', bodyClassName)}>
        {kpis && (
          <KpiStrip
            items={kpis}
            className={cn(
              'gap-[12px] xl:grid-cols-4 [&>div]:border-0 [&>div]:bg-[var(--functional-surface)] [&>div]:[box-shadow:var(--shadow-functional-surface)]',
              kpiClassName,
            )}
          />
        )}
        {filterBar && (
          <div className="flex flex-wrap items-end gap-3 rounded-md bg-[var(--functional-surface)] p-[12px] [box-shadow:var(--shadow-functional-surface)]">
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
      </SurfacePageBody>
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
    <div className="grid gap-1">
      <span className="type-caption font-sans font-strong text-tx-3">{label}</span>
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger
          aria-label={label}
          className="h-8 w-auto min-w-[132px] px-2.5 py-0 text-xs font-strong text-tx-1"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent align="start">
          {options.map((option) => (
            <SelectItem key={option.value} value={option.value} className="text-xs">
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
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
    <div className="flex min-w-0 flex-wrap items-end justify-between gap-3">
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
}: {
  performance?: boolean | undefined;
  settings?: boolean | undefined;
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
    </ModuleTabs>
  );
}

export function useRumBasePath(): '/rum' {
  return '/rum';
}
