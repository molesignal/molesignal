import type { LucideIcon } from 'lucide-react';
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
import { ViewCountsUnknownContext } from '@/shell/tabs/ViewTabs';
import { useSubnavStore } from '@/stores/useSubnavStore';

import type { ProductBreadcrumbItem } from './ia';
import { listContent } from './listContent';
import { SectionsAside, SectionsExpand } from './SectionsAside';
import { ProductState, type ProductStateProps } from './states';

interface ProductPageFrameProps {
  title: React.ReactNode;
  subtitle?: string | undefined;
  toolbar?: React.ReactNode | undefined;
  breadcrumbs?: readonly ProductBreadcrumbItem[] | null | undefined;
  backTo?: string | null | undefined;
  /** Optional row of tabs between the header and the body (a module's `ModuleTabs`). */
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
  /** The module's pages in a menu beside the list, grouped, instead of a row of tabs over it (Alerts). */
  sections?: React.ReactNode | undefined;
  /** How the menu folds away for a wide page, held by the page as Access holds its own. */
  sectionNavigation?: SectionNavigation | undefined;
  kpis?: readonly KpiStripItem[] | undefined;
  kpiLayout?: KpiStripLayout | undefined;
  kpiClassName?: string | undefined;
  filters?: React.ReactNode | undefined;
  filterClassName?: string | undefined;
  /**
   * The page's opening statement, a surface of its own above the list: what the
   * list amounts to, said once. It takes the place of a strip of number cards.
   */
  lead?: React.ReactNode | undefined;
  /**
   * What the list is showing: its view, a search, its filters, set at the head
   * of the list itself instead of in a card of their own above it. The head
   * stays when the list is empty, so a filter that matches nothing can be
   * taken back where it was set. Surface pages only.
   */
  listHead?: React.ReactNode | undefined;
  actionBar?: React.ReactNode | undefined;
  state?: ProductStateProps | null | undefined;
  stateClassName?: string | undefined;
  /**
   * Wrap the content in one surface (the default, for a table). A page whose
   * content is already several surfaces turns it off, so they sit on the canvas
   * instead of inside one more card.
   */
  contentSurface?: boolean | undefined;
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
  /**
   * The menu draws its own collapse control beside its search field (as the
   * Settings and Access menus do), so the page does not float a second one over
   * the field. The page still owns the collapsed strip and its expand control.
   */
  menuOwnsCollapse?: boolean | undefined;
  state?: ProductStateProps | null | undefined;
  sectionNavigation?: SectionNavigation | undefined;
}

/** A menu of sections the page folds away itself: folded, it leaves no strip beside the content. */
interface SectionNavigation {
  collapsed: boolean;
  onExpand: () => void;
  expandLabel: string;
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

/** A figure nobody has answered yet: not a zero. */
function unknownFigure(item: KpiStripItem): KpiStripItem {
  return { ...item, value: '—', tone: 'neutral' };
}

export function ListPage({
  sections,
  sectionNavigation,
  kpis,
  kpiLayout,
  kpiClassName,
  filters,
  filterClassName,
  lead,
  listHead,
  actionBar,
  state: requestedState,
  stateClassName,
  contentSurface = true,
  children: list,
  bodyClassName,
  appearance = 'default',
  ...frame
}: ListPageProps) {
  const surface = appearance === 'surface';
  const headed = surface && contentSurface && Boolean(listHead);
  const figuresUnknown = requestedState?.variant === 'loading' || requestedState?.variant === 'error';
  const { state, children } = listContent(requestedState, list);
  const stack = surface ? 'space-y-[8px]' : 'space-y-4';
  const folded = Boolean(sections && sectionNavigation?.collapsed);
  const body = (
    <>
      <KpiStrip
        items={figuresUnknown ? kpis?.map(unknownFigure) : kpis}
        layout={kpiLayout}
        className={cn(
          surface
            && 'gap-[8px] [&>div]:border-0 [&>div]:bg-[var(--functional-surface)] [&>div]:[box-shadow:var(--shadow-functional-surface)]',
          kpiClassName,
        )}
      />
      {lead}
      <FilterArea
        className={cn(
          surface
            ? 'rounded-md border-0 bg-[var(--functional-surface)] px-[12px] py-[8px] [box-shadow:var(--shadow-functional-surface)]'
            : undefined,
          filterClassName,
        )}
      >
        {filters}
      </FilterArea>
      <ActionBar
        className={cn(
          surface
            && 'rounded-md border-0 bg-[var(--functional-surface)] px-[12px] py-[8px] [box-shadow:var(--shadow-functional-surface)]',
        )}
      >
        {actionBar}
      </ActionBar>
      {headed ? (
        <div
          data-list-surface
          className="min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]"
        >
          <div
            data-list-head
            className="flex min-h-[48px] min-w-0 flex-wrap items-center gap-x-[12px] gap-y-[8px] border-b border-bd-0 px-[12px] py-[8px]"
          >
            {listHead}
          </div>
          {state ? (
            <ProductState
              {...state}
              className={cn(
                'rounded-none border-0 bg-transparent shadow-none',
                stateClassName,
                state.className,
              )}
            />
          ) : (
            children
          )}
        </div>
      ) : state ? (
        <ProductState
          {...state}
          className={cn(
            surface
              && 'rounded-md border-0 bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
            stateClassName,
            state.className,
          )}
        />
      ) : surface && contentSurface ? (
        <div
          data-list-surface
          className="min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]"
        >
          {children}
        </div>
      ) : (
        children
      )}
    </>
  );

  return (
    <ViewCountsUnknownContext.Provider value={figuresUnknown}>
      <ProductPageFrame
        {...frame}
        appearance={appearance}
        bodyClassName={cn(
          sections
            ? cn(
                'grid items-start',
                surface ? 'gap-[8px]' : 'gap-6',
                folded ? 'lg:grid-cols-1' : 'lg:grid-cols-[var(--subsidebar-w)_minmax(0,1fr)]',
              )
            : stack,
          bodyClassName,
        )}
      >
        {sections ? (
          <>
            <SectionsAside
              sections={sections}
              appearance={appearance}
              {...(sectionNavigation
                ? {
                    folding: {
                      collapsed: sectionNavigation.collapsed,
                      fullyHidden: true,
                      ownsCollapse: true,
                      onToggle: sectionNavigation.onExpand,
                      expandLabel: sectionNavigation.expandLabel,
                      collapseLabel: sectionNavigation.expandLabel,
                    },
                  }
                : {})}
            />
            <div className="relative min-w-0">
              {folded && sectionNavigation && (
                <SectionsExpand label={sectionNavigation.expandLabel} onExpand={sectionNavigation.onExpand} />
              )}
              <div className={stack}>{body}</div>
            </div>
          </>
        ) : (
          body
        )}
      </ProductPageFrame>
    </ViewCountsUnknownContext.Provider>
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
        // In the page's gutter, as its title and its body are, and a card's distance over the body.
        className={cn(
          'mx-[20px] mb-[12px]',
          surface && 'rounded-md border-0 bg-[var(--functional-surface)] px-[12px] [box-shadow:var(--shadow-functional-surface)]',
        )}
      />
      {surface ? (
        <SurfacePageBody className={cn('space-y-[8px]', bodyClassName)}>
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
  menuOwnsCollapse = false,
  state: requestedState,
  children: content,
  bodyClassName,
  sectionNavigation,
  appearance = 'default',
  ...frame
}: ManagementPageProps) {
  const { t } = useTranslation('shell');
  const { state, children } = listContent(requestedState, content);
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
        appearance === 'surface' ? 'gap-[8px]' : 'gap-6',
        sections &&
          (collapsed && fullyHidden
            ? 'lg:grid-cols-1'
            : collapsed
              ? 'lg:grid-cols-[auto_minmax(0,1fr)]'
              : 'lg:grid-cols-[var(--subsidebar-w)_minmax(0,1fr)]'),
        bodyClassName,
      )}
    >
      {sections && (
        <SectionsAside
          sections={sections}
          appearance={appearance}
          folding={{
            collapsed,
            fullyHidden,
            ownsCollapse: menuOwnsCollapse,
            onToggle: toggle,
            expandLabel,
            collapseLabel: t('subnav.collapse', { defaultValue: 'Collapse menu' }),
          }}
        />
      )}
      <div
        data-management-content
        data-management-appearance={appearance}
        data-sections-collapsed={collapsed && fullyHidden ? 'true' : 'false'}
        className={cn(
          'relative min-w-0',
          appearance === 'surface' &&
            'rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        )}
      >
        {collapsed && fullyHidden && <SectionsExpand label={expandLabel} onExpand={toggle} />}
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
