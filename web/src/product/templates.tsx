import { PanelLeftClose, PanelLeftOpen, type LucideIcon } from 'lucide-react';
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
  surfacePageRootClass,
  SurfacePageBody,
  SurfacePageHeader,
} from '@/shell/SurfaceWorkbench';
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
  headerCompact?: boolean | undefined;
  headerIcon?: LucideIcon | null | undefined;
  bodyClassName?: string | undefined;
  padded?: boolean | undefined;
  appearance?: 'default' | 'surface' | undefined;
}

interface OverviewPageProps extends ProductPageFrameProps {
  kpis?: readonly KpiStripItem[] | undefined;
  aside?: React.ReactNode | undefined;
}

interface ListPageProps extends ProductPageFrameProps {
  kpis?: readonly KpiStripItem[] | undefined;
  kpiLayout?: KpiStripLayout | undefined;
  kpiClassName?: string | undefined;
  cardless?: boolean | undefined;
  filters?: React.ReactNode | undefined;
  filterClassName?: string | undefined;
  actionBar?: React.ReactNode | undefined;
  state?: ProductStateProps | null | undefined;
  stateClassName?: string | undefined;
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
  cardless = false,
  filters,
  filterClassName,
  actionBar,
  state,
  stateClassName,
  children,
  bodyClassName,
  appearance = 'default',
  ...frame
}: ListPageProps) {
  const surface = appearance === 'surface';
  return (
    <ProductPageFrame
      {...frame}
      appearance={appearance}
      bodyClassName={cn(
        cardless ? 'space-y-0' : surface ? 'space-y-[12px]' : 'space-y-4',
        bodyClassName,
      )}
    >
      <KpiStrip
        items={kpis}
        layout={kpiLayout}
        className={cn(
          cardless
            && 'gap-0 border-b border-bd-0 bg-bg-0 [&>div]:min-h-[92px] [&>div]:rounded-none [&>div]:border-0 [&>div]:bg-transparent [&>div]:px-4 [&>div]:py-3',
          surface
            && !cardless
            && 'gap-[12px] [&>div]:border-0 [&>div]:bg-[var(--functional-surface)] [&>div]:[box-shadow:var(--shadow-functional-surface)]',
          kpiClassName,
        )}
      />
      <FilterArea
        className={cn(
          cardless
            ? 'rounded-none border-x-0 border-t-0 bg-transparent p-0 px-4 py-2'
            : surface
              ? 'rounded-md border-0 bg-[var(--functional-surface)] p-[12px] [box-shadow:var(--shadow-functional-surface)]'
            : undefined,
          filterClassName,
        )}
      >
        {filters}
      </FilterArea>
      <ActionBar
        className={cn(
          cardless && 'bg-transparent px-4 py-2',
          surface
            && !cardless
            && 'rounded-md border-0 bg-[var(--functional-surface)] px-[12px] py-[8px] [box-shadow:var(--shadow-functional-surface)]',
        )}
      >
        {actionBar}
      </ActionBar>
      {state ? (
        <ProductState
          {...state}
          className={cn(
            cardless
              && 'rounded-none border-x-0 border-t-0 border-solid border-bd-0 bg-transparent',
            surface
              && !cardless
              && 'rounded-md border-0 bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
            stateClassName,
            state.className,
          )}
        />
      ) : surface && !cardless ? (
        <div
          data-list-surface
          className="min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]"
        >
          {children}
        </div>
      ) : (
        children
      )}
    </ProductPageFrame>
  );
}

export function DetailPage({
  metadata,
  state,
  children,
  bodyClassName,
  appearance = 'default',
  ...frame
}: DetailPageProps) {
  const surface = appearance === 'surface';
  return (
    <ProductPageFrame {...frame} appearance={appearance} padded={false}>
      <MetadataStrip
        items={metadata}
        className={surface
          ? 'mx-[20px] mb-[12px] rounded-md border-0 bg-[var(--functional-surface)] px-[12px] [box-shadow:var(--shadow-functional-surface)]'
          : undefined}
      />
      {surface ? (
        <SurfacePageBody className={cn('space-y-[12px]', bodyClassName)}>
          {state ? <ProductState {...state} /> : children}
        </SurfacePageBody>
      ) : (
        <PageBody className={cn('space-y-6', bodyClassName)}>
          {state ? <ProductState {...state} /> : children}
        </PageBody>
      )}
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
  appearance = 'default',
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
      appearance={appearance}
      bodyClassName={cn(
        'grid',
        appearance === 'surface' ? 'gap-[12px]' : 'gap-6',
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
              className={cn(
                'sticky top-6 hidden h-9 w-9 place-items-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 lg:grid',
                appearance === 'surface'
                  ? 'border-0 bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]'
                  : 'border border-bd-0 bg-bg-1',
              )}
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
            <div
              className={cn(
                appearance === 'surface' &&
                  'rounded-md bg-[var(--functional-surface)] p-[12px] [box-shadow:var(--shadow-functional-surface)]',
              )}
            >
              {sections}
            </div>
          </aside>
        ))}
      <div
        data-management-content
        data-sections-collapsed={collapsed && fullyHidden ? 'true' : 'false'}
        className={cn(
          'relative min-w-0',
          appearance === 'surface' &&
            'rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        )}
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
  headerCompact,
  headerIcon,
  bodyClassName,
  padded = true,
  appearance = 'default',
}: ProductPageFrameProps) {
  const surface = appearance === 'surface';
  const headerProps = {
    title,
    subtitle,
    toolbar,
    breadcrumbs,
    backTo,
    className: headerClassName,
    compact: headerCompact,
    moduleIcon: headerIcon,
  };

  return (
    <div
      data-page-appearance={appearance}
      className={cn(surface && surfacePageRootClass, className)}
    >
      {surface ? (
        <SurfacePageHeader {...headerProps} />
      ) : (
        <PageHeader {...headerProps} />
      )}
      {subnav}
      {padded ? (
        surface ? (
          <SurfacePageBody className={bodyClassName}>{children}</SurfacePageBody>
        ) : (
          <PageBody className={bodyClassName}>{children}</PageBody>
        )
      ) : (
        children
      )}
    </div>
  );
}
