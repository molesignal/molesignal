import { type LucideIcon, RefreshCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';

/**
 * A card of the overview: a head row with its title and what it counts, and what it shows. An icon
 * in the colour of what the card is about leads its title, as it does in the other modules' cards.
 */
export function OverviewCard({
  title,
  icon: Icon,
  tone = 'text-[var(--chart-1)]',
  description,
  action,
  testId,
  className,
  bodyClassName,
  children,
}: {
  title: React.ReactNode;
  icon?: LucideIcon | undefined;
  /** The ink of the icon, a text colour class. */
  tone?: string | undefined;
  description?: React.ReactNode;
  /** What the card offers beside its title, at the far end of its head: a way to the whole of it. */
  action?: React.ReactNode;
  testId: string;
  className?: string | undefined;
  bodyClassName?: string | undefined;
  children: React.ReactNode;
}) {
  return (
    <section data-rum-overview-card={testId} className={cn(surfacePanelClass, 'min-w-0 overflow-hidden', className)}>
      <header className="flex min-h-10 items-center gap-3 border-b border-bd-0 px-3 py-[5px]">
        <div className="min-w-0 flex-1">
          <h2 className="m-0 flex items-center gap-1.5 type-section-title font-sans font-display text-tx-0">
            {Icon && <Icon aria-hidden className={cn('h-4 w-4 shrink-0', tone)} />}
            <span className="min-w-0 truncate">{title}</span>
          </h2>
          {description && <p className="m-0 mt-0.5 truncate font-sans text-xs text-tx-3">{description}</p>}
        </div>
        {action}
      </header>
      <div className={bodyClassName}>{children}</div>
    </section>
  );
}

/** What a read of a card shows until it has numbers: loading, an error to retry, why there are none, or nothing found. */
export function CardState({
  query,
  empty,
  emptyTitle,
  children,
}: {
  query: {
    isPending: boolean;
    isError: boolean;
    error: unknown;
    refetch: () => unknown;
    /** What was read; only the reason it has no numbers matters here. */
    data: { unavailable?: RumAggregate['unavailable'] | undefined } | undefined;
  };
  empty: boolean;
  emptyTitle: React.ReactNode;
  children: React.ReactNode;
}) {
  const { t } = useTranslation('rum');

  if (query.isPending) return <ProductState variant="loading" compact />;

  if (query.isError) {
    const status = (query.error as { response?: { status?: number } } | null)?.response?.status;

    return (
      <ProductState
        variant={status === 403 ? 'permission-denied' : 'error'}
        error={query.error}
        compact
        action={
          status === 403 ? undefined : (
            <button
              type="button"
              onClick={() => void query.refetch()}
              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-[var(--control-surface)] px-3 font-sans text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
            >
              <RefreshCw aria-hidden className="h-3.5 w-3.5" />
              {t('overview.retry')}
            </button>
          )
        }
      />
    );
  }

  const unavailable = query.data?.unavailable;

  // The title says it all: the state's own line is about creating a first item.
  if (unavailable) return <ProductState variant="empty" compact title={t(`overview.unavailable.${unavailable}`)} description="" />;

  if (empty) return <ProductState variant="empty" compact title={emptyTitle} description="" />;

  return <>{children}</>;
}
