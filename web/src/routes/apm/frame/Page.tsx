import { type LucideIcon, RefreshCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ProductBreadcrumbItem } from '@/product/ia';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { surfacePageRootClass, SurfacePageBody, SurfacePageHeader } from '@/shell/SurfaceWorkbench';

/**
 * An APM page: its title, its navigation card (the module's, or a service's
 * own), and its body. The module keeps one title from tab to tab; a service
 * page is titled with the service.
 */
export function ApmPage({
  title,
  subtitle,
  toolbar,
  breadcrumbs,
  navigation,
  page,
  children,
}: {
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  /** What acts on the page's subject, at the end of its title row. */
  toolbar?: React.ReactNode;
  breadcrumbs?: readonly ProductBreadcrumbItem[] | null | undefined;
  navigation: React.ReactNode;
  /** Which page, for tests and styles: `catalog`, `service`, … */
  page: string;
  children: React.ReactNode;
}) {
  return (
    <div data-surface-workbench-page="apm" data-apm-page={page} className={surfacePageRootClass}>
      <SurfacePageHeader title={title} subtitle={subtitle} toolbar={toolbar} breadcrumbs={breadcrumbs} />
      {navigation}
      <SurfacePageBody className="space-y-[12px]">{children}</SurfacePageBody>
    </div>
  );
}

/**
 * Where a card stands: on a page, its title is a section title over the 12px
 * body; inside a drawer, whose own title leads, it is a step smaller. A title
 * as small as the body it heads makes the whole page read as one block of text.
 */
const ApmCardLevel = React.createContext<'page' | 'drawer'>('page');

export function ApmDrawerCards({ children }: { children: React.ReactNode }) {
  return <ApmCardLevel.Provider value="drawer">{children}</ApmCardLevel.Provider>;
}

const CARD_TITLE = {
  page: 'type-section-title truncate font-sans font-display text-tx-0',
  drawer: 'truncate font-sans text-md font-display text-tx-0',
} as const;

/**
 * A card of a page: a head row with its title and actions, and what it shows.
 * An icon in the colour of what the card is about leads its title.
 */
export function ApmCard({
  title,
  icon: Icon,
  tone = 'text-[var(--chart-1)]',
  description,
  actions,
  className,
  bodyClassName,
  testId,
  children,
}: {
  title?: React.ReactNode;
  icon?: LucideIcon;
  /** The ink of the icon, a text colour class. */
  tone?: string;
  description?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string | undefined;
  bodyClassName?: string | undefined;
  testId?: string | undefined;
  children: React.ReactNode;
}) {
  const level = React.useContext(ApmCardLevel);

  return (
    <section
      data-apm-card={testId}
      className={cn('min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]', className)}
    >
      {(title || actions) && (
        <header className="flex min-h-10 items-center gap-3 border-b border-bd-0 px-3 py-[5px]">
          <div className="min-w-0 flex-1">
            {title && (
              <h2 className={cn(CARD_TITLE[level], Icon && 'flex items-center gap-1.5')}>
                {Icon && <Icon aria-hidden className={cn('shrink-0', level === 'page' ? 'h-4 w-4' : 'h-3.5 w-3.5', tone)} />}
                {Icon ? <span className="min-w-0 truncate">{title}</span> : title}
              </h2>
            )}
            {description && <p className="mt-0.5 truncate font-sans text-xs text-tx-3">{description}</p>}
          </div>
          {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
        </header>
      )}
      <div className={bodyClassName}>{children}</div>
    </section>
  );
}

/** What a query's part of a page shows until it has data: loading, an error to retry, or nothing found. */
export function ApmQueryState({
  query,
  empty,
  emptyTitle,
  emptyDescription,
  compact = false,
  children,
}: {
  query: {
    isPending: boolean;
    isError: boolean;
    error: unknown;
    refetch: () => unknown;
  };
  empty: boolean;
  emptyTitle?: React.ReactNode;
  emptyDescription?: React.ReactNode;
  compact?: boolean;
  children: React.ReactNode;
}) {
  const { t } = useTranslation('apm');

  if (query.isPending) return <ProductState variant="loading" compact={compact} />;

  if (query.isError) {
    const status = (query.error as { response?: { status?: number } } | null)?.response?.status;

    return (
      <ProductState
        variant={status === 403 ? 'permission-denied' : 'error'}
        error={query.error}
        compact={compact}
        action={
          status === 403 ? undefined : (
            <button
              type="button"
              onClick={() => void query.refetch()}
              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-[var(--control-surface)] px-3 font-sans text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
            >
              <RefreshCw aria-hidden className="h-3.5 w-3.5" />
              {t('actions.retry')}
            </button>
          )
        }
      />
    );
  }

  if (empty) {
    return <ProductState variant="empty" compact={compact} title={emptyTitle ?? t('states.empty')} description={emptyDescription} />;
  }

  return <>{children}</>;
}
