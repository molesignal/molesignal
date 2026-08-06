import { PanelLeftClose, PanelLeftOpen } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import {
  ActionBar,
  FilterArea,
  KpiStrip,
  type KpiStripLayout,
  type KpiStripItem,
  MetadataStrip,
  type MetadataStripItem,
} from '@/admin';
import { cn } from '@/shell/lib/cn';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';
import { useSubnavStore } from '@/stores/useSubnavStore';

import type { ProductBreadcrumbItem } from './ia';
import { ProductState, type ProductStateProps } from './states';

interface ProductPageFrameProps {
  title: React.ReactNode;
  subtitle?: string | undefined;
  toolbar?: React.ReactNode | undefined;
  breadcrumbs?: readonly ProductBreadcrumbItem[] | null | undefined;
  backTo?: string | null | undefined;
  /** Optional sub-nav rendered between the header and the body (e.g. AlertsSubNav). */
  subnav?: React.ReactNode | undefined;
  children?: React.ReactNode | undefined;
  className?: string | undefined;
  headerClassName?: string | undefined;
  bodyClassName?: string | undefined;
  padded?: boolean | undefined;
}

interface OverviewPageProps extends ProductPageFrameProps {
  kpis?: readonly KpiStripItem[] | undefined;
  aside?: React.ReactNode | undefined;
}

interface ListPageProps extends ProductPageFrameProps {
  kpis?: readonly KpiStripItem[] | undefined;
  kpiLayout?: KpiStripLayout | undefined;
  kpiClassName?: string | undefined;
  filters?: React.ReactNode | undefined;
  actionBar?: React.ReactNode | undefined;
  state?: ProductStateProps | null | undefined;
}

interface DetailPageProps extends ProductPageFrameProps {
  metadata?: readonly MetadataStripItem[] | undefined;
  state?: ProductStateProps | null | undefined;
}

interface BuilderPageProps extends ProductPageFrameProps {
  palette?: React.ReactNode | undefined;
  inspector?: React.ReactNode | undefined;
  state?: ProductStateProps | null | undefined;
  paletteClassName?: string | undefined;
  mainClassName?: string | undefined;
  inspectorClassName?: string | undefined;
}

interface ManagementPageProps extends ProductPageFrameProps {
  sections?: React.ReactNode | undefined;
  state?: ProductStateProps | null | undefined;
  sectionNavigation?:
    | {
        collapsed: boolean;
        onExpand: () => void;
        expandLabel: string;
      }
    | undefined;
}

interface GatePageProps extends Omit<ProductPageFrameProps, 'children'> {
  state: ProductStateProps;
}

export function OverviewPage({ kpis, aside, children, bodyClassName, ...frame }: OverviewPageProps) {
  return (
    <ProductPageFrame {...frame} bodyClassName={cn('flex flex-col gap-6', bodyClassName)}>
      <KpiStrip items={kpis} />
      <div className={cn('grid min-h-0 flex-1 gap-6', aside && 'xl:grid-cols-[minmax(0,1fr)_360px]')}>
        <div className="min-h-0 min-w-0">{children}</div>
        {aside && <aside className="min-h-0 min-w-0">{aside}</aside>}
      </div>
    </ProductPageFrame>
  );
}

export function ListPage({
  kpis,
  kpiLayout,
  kpiClassName,
  filters,
  actionBar,
  state,
  children,
  bodyClassName,
  ...frame
}: ListPageProps) {
  return (
    <ProductPageFrame {...frame} bodyClassName={cn('space-y-4', bodyClassName)}>
      <KpiStrip items={kpis} layout={kpiLayout} className={kpiClassName} />
      <FilterArea>{filters}</FilterArea>
      <ActionBar>{actionBar}</ActionBar>
      {state ? <ProductState {...state} /> : children}
    </ProductPageFrame>
  );
}

export function DetailPage({ metadata, state, children, bodyClassName, ...frame }: DetailPageProps) {
  return (
    <ProductPageFrame {...frame} padded={false}>
      <MetadataStrip items={metadata} />
      <PageBody className={cn('space-y-6', bodyClassName)}>
        {state ? <ProductState {...state} /> : children}
      </PageBody>
    </ProductPageFrame>
  );
}

export function BuilderPage({
  palette,
  inspector,
  state,
  children,
  bodyClassName,
  paletteClassName,
  mainClassName,
  inspectorClassName,
  ...frame
}: BuilderPageProps) {
  return (
    <ProductPageFrame {...frame} padded={false}>
      <div
        className={cn(
          'grid min-h-[calc(100vh-var(--topbar-h)-var(--pageheader-h,0px)-var(--contextbar-h,0px))] grid-cols-1 bg-bg-0 lg:grid-cols-[240px_minmax(0,1fr)_360px]',
          bodyClassName,
        )}
      >
        {palette && (
          <aside className={cn('min-h-0 border-b border-bd-0 bg-bg-1 p-4 lg:border-b-0 lg:border-r', paletteClassName)}>
            {palette}
          </aside>
        )}
        <main className={cn('min-h-0 min-w-0 p-6', mainClassName)}>
          {state ? <ProductState {...state} /> : children}
        </main>
        {inspector && (
          <aside className={cn('min-h-0 border-t border-bd-0 bg-bg-1 p-4 lg:border-l lg:border-t-0', inspectorClassName)}>
            {inspector}
          </aside>
        )}
      </div>
    </ProductPageFrame>
  );
}

export function ManagementPage({
  sections,
  state,
  children,
  bodyClassName,
  sectionNavigation,
  ...frame
}: ManagementPageProps) {
  const { t } = useTranslation('shell');
  const storedCollapsed = useSubnavStore((s) => s.collapsed);
  const storedToggle = useSubnavStore((s) => s.toggle);
  const fullyHidden = sectionNavigation !== undefined;
  const collapsed = sectionNavigation?.collapsed ?? storedCollapsed;
  const toggle = sectionNavigation?.onExpand ?? storedToggle;
  const expandLabel =
    sectionNavigation?.expandLabel ??
    t('subnav.expand', { defaultValue: 'Expand menu' });
  return (
    <ProductPageFrame
      {...frame}
      bodyClassName={cn(
        'grid gap-6',
        sections &&
          (collapsed && fullyHidden
            ? 'lg:grid-cols-1'
            : collapsed
              ? 'lg:grid-cols-[auto_minmax(0,1fr)]'
              : 'lg:grid-cols-[var(--subsidebar-w)_minmax(0,1fr)]'),
        bodyClassName,
      )}
    >
      {sections &&
        (collapsed && !fullyHidden ? (
          // Collapsed: a thin strip with just an expand affordance.
          <aside>
            <button
              type="button"
              onClick={toggle}
              aria-label={expandLabel}
              title={expandLabel}
              className="sticky top-6 hidden h-9 w-9 place-items-center rounded-md border border-bd-0 bg-bg-1 text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 lg:grid"
            >
              <PanelLeftOpen className="h-4 w-4" />
            </button>
            <div className="lg:hidden">{sections}</div>
          </aside>
        ) : (
          <aside
            className={cn(
              'relative min-w-0',
              collapsed && fullyHidden && 'lg:hidden',
            )}
          >
            {!fullyHidden && (
              <button
                type="button"
                onClick={toggle}
                aria-label={t('subnav.collapse', {
                  defaultValue: 'Collapse menu',
                })}
                title={t('subnav.collapse', {
                  defaultValue: 'Collapse menu',
                })}
                className="absolute right-2 top-2 z-10 hidden h-8 w-8 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 lg:grid"
              >
                <PanelLeftClose className="h-3.5 w-3.5" />
              </button>
            )}
            {sections}
          </aside>
        ))}
      <div
        data-management-content
        data-sections-collapsed={collapsed && fullyHidden ? 'true' : 'false'}
        className="relative min-w-0"
      >
        {collapsed && fullyHidden && (
          <div className="absolute -left-5 top-0 z-10 hidden h-full lg:block">
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  onClick={toggle}
                  aria-label={expandLabel}
                  className="sticky top-6 grid h-8 w-8 place-items-center rounded-md bg-bg-0 text-tx-3 opacity-50 transition-colors hover:bg-bg-2 hover:text-tx-0 hover:opacity-100 focus-visible:bg-bg-2 focus-visible:text-tx-0 focus-visible:opacity-100"
                >
                  <PanelLeftOpen className="h-3.5 w-3.5" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="right">{expandLabel}</TooltipContent>
            </Tooltip>
          </div>
        )}
        {state ? <ProductState {...state} /> : children}
      </div>
    </ProductPageFrame>
  );
}

/**
 * Backwards-compatible name for the settings fixture and any downstream
 * consumers. IAM and Settings use ManagementPage directly to make the shared
 * management-center shell explicit.
 */
export function SettingsPage(props: ManagementPageProps) {
  return <ManagementPage {...props} />;
}

export function GatePage({ state, bodyClassName, ...frame }: GatePageProps) {
  return (
    <ProductPageFrame {...frame} bodyClassName={bodyClassName}>
      <ProductState {...state} />
    </ProductPageFrame>
  );
}

function ProductPageFrame({
  title,
  subtitle,
  toolbar,
  breadcrumbs,
  backTo,
  subnav,
  children,
  className,
  headerClassName,
  bodyClassName,
  padded = true,
}: ProductPageFrameProps) {
  return (
    <div className={className}>
      <PageHeader
        title={title}
        subtitle={subtitle}
        toolbar={toolbar}
        breadcrumbs={breadcrumbs}
        backTo={backTo}
        className={headerClassName}
      />
      {subnav}
      {padded ? (
        <PageBody className={bodyClassName}>{children}</PageBody>
      ) : (
        children
      )}
    </div>
  );
}
