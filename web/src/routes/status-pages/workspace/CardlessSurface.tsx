import type * as React from 'react';

import type { KpiStripItem } from '@/admin';
import { uiLabelClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';

const KPI_GRID_CLASS = {
  3: 'sm:grid-cols-3',
  4: 'sm:grid-cols-2 xl:grid-cols-4',
  5: 'sm:grid-cols-2 xl:grid-cols-5',
} as const;

export const statusPageFlatTableClassName = 'rounded-none border-0 bg-transparent';
export const statusPageFlatStateClassName =
  'rounded-none border-0 bg-transparent shadow-none';

export function StatusPageCanvas({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      data-status-page-canvas
      className={cn('mx-auto w-full max-w-[2200px] space-y-[12px] bg-[var(--page-canvas)]', className)}
    >
      {children}
    </div>
  );
}

export function StatusPageKpiBand({
  items,
  columns = 4,
  className,
}: {
  items: readonly KpiStripItem[];
  columns?: keyof typeof KPI_GRID_CLASS;
  className?: string;
}) {
  if (items.length === 0) return null;

  return (
    <section
      data-status-page-kpis
      className={cn(
        'grid grid-cols-1 gap-[12px]',
        KPI_GRID_CLASS[columns],
        className,
      )}
    >
      {items.map((item, index) => (
        <div key={index} className="min-h-[92px] min-w-0 rounded-md bg-[var(--functional-surface)] px-4 py-3 [box-shadow:var(--shadow-functional-surface)]">
          <div className={uiLabelClass}>{item.label}</div>
          <div
            className={cn(
              'mt-2 truncate font-sans text-2xl font-display-strong leading-none tracking-[-0.025em] tabular-nums',
              (!item.tone || item.tone === 'neutral') && 'text-tx-0',
              item.tone === 'good' && 'text-green',
              item.tone === 'warn' && 'text-yellow',
              item.tone === 'danger' && 'text-red',
            )}
          >
            {item.value}
          </div>
          {item.sub && <div className="mt-1.5 truncate text-xs text-tx-2">{item.sub}</div>}
        </div>
      ))}
    </section>
  );
}

export function StatusPageBand({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      data-status-page-band
      className={cn('min-h-12 rounded-md bg-[var(--functional-surface)] px-4 py-2 [box-shadow:var(--shadow-functional-surface)]', className)}
    >
      {children}
    </div>
  );
}

export function StatusPageFilterBand({
  children,
  className,
  ...props
}: React.ComponentProps<'form'>) {
  return (
    <form
      data-status-page-filter-band
      className={cn('min-h-12 rounded-md bg-[var(--functional-surface)] px-4 py-2 [box-shadow:var(--shadow-functional-surface)]', className)}
      {...props}
    >
      {children}
    </form>
  );
}

export function StatusPageListSurface({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section
      data-status-page-list-surface
      className={cn('min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]', className)}
    >
      {children}
    </section>
  );
}

export function StatusPageSection({
  title,
  description,
  action,
  children,
  className,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  action?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section
      data-status-page-section
      className={cn('min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]', className)}
    >
      <div className="flex min-h-12 items-center justify-between gap-4 px-4 py-2.5">
        <div className="min-w-0">
          <h2 className="text-sm font-display-strong text-tx-0">{title}</h2>
          {description && <p className="mt-0.5 text-xs text-tx-2">{description}</p>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}
