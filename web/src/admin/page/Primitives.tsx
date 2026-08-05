import * as React from 'react';

import { uiLabelClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';

export interface KpiStripItem {
  label: React.ReactNode;
  value: React.ReactNode;
  sub?: React.ReactNode | undefined;
  tone?: 'neutral' | 'good' | 'warn' | 'danger' | undefined;
}

export interface MetadataStripItem {
  label: React.ReactNode;
  value: React.ReactNode;
}

export type KpiStripLayout = 'grid' | 'inline';

export function PageToolbar({
  children,
  className,
}: {
  children?: React.ReactNode | undefined;
  className?: string | undefined;
}) {
  if (!children) return null;
  return (
    <div className={cn('flex min-h-11 flex-wrap items-center gap-2 border-b border-bd-0 bg-bg-1 px-6 py-3', className)}>
      {children}
    </div>
  );
}

export function KpiStrip({
  items,
  children,
  className,
  layout = 'grid',
}: {
  items?: readonly KpiStripItem[] | undefined;
  children?: React.ReactNode | undefined;
  className?: string | undefined;
  layout?: KpiStripLayout | undefined;
}) {
  if ((!items || items.length === 0) && !children) return null;
  return (
    <div
      className={cn(
        'grid w-full gap-3 overflow-visible',
        layout === 'grid' && 'sm:grid-cols-2 xl:grid-cols-4',
        layout === 'inline' && 'overflow-x-auto overflow-y-hidden',
        className,
      )}
      style={
        layout === 'inline' && items?.length
          ? { gridTemplateColumns: `repeat(${items.length}, minmax(168px, 1fr))` }
          : undefined
      }
    >
      {items?.map((item, index) => (
        <KpiStripStat key={index} {...item} />
      ))}
      {children}
    </div>
  );
}

export function KpiStripStat({ label, value, sub, tone = 'neutral' }: KpiStripItem) {
  return (
    <div className="min-h-[112px] rounded-lg border border-bd-0 bg-bg-1 px-5 py-4">
      <div className={uiLabelClass}>{label}</div>
      <div
        className={cn(
          // Review-calibrated KPI: 32px display-strong, tabular-nums.
          // Stronger than the surrounding 13.5px body so a glance picks
          // the most important number out of the strip.
          'mt-2 font-sans [font-size:32px] font-display-strong leading-none tracking-[-0.025em] tabular-nums',
          tone === 'neutral' && 'text-tx-0',
          tone === 'good' && 'text-green',
          // Yellow is the warning status color in Phase 4 — orange
          // shifted to brand-secondary.
          tone === 'warn' && 'text-yellow',
          tone === 'danger' && 'text-red',
        )}
      >
        {value}
      </div>
      {sub && <div className="mt-2 truncate font-sans text-sm text-tx-2">{sub}</div>}
    </div>
  );
}

export function ActionBar({
  children,
  className,
}: {
  children?: React.ReactNode | undefined;
  className?: string | undefined;
}) {
  if (!children) return null;
  return (
    <div className={cn('flex min-h-11 flex-wrap items-center justify-between gap-2 border-b border-bd-0 bg-bg-1 px-6 py-3', className)}>
      {children}
    </div>
  );
}

export function FilterArea({
  children,
  className,
}: {
  children?: React.ReactNode | undefined;
  className?: string | undefined;
}) {
  if (!children) return null;
  return (
    <div className={cn('flex flex-wrap items-center gap-2 rounded-lg border border-bd-0 bg-bg-1 p-3', className)}>
      {children}
    </div>
  );
}

export function MetadataStrip({
  items,
  children,
  className,
}: {
  items?: readonly MetadataStripItem[] | undefined;
  children?: React.ReactNode | undefined;
  className?: string | undefined;
}) {
  if ((!items || items.length === 0) && !children) return null;
  return (
    <dl className={cn('flex flex-wrap items-center gap-x-5 gap-y-2 border-b border-bd-0 bg-bg-1 px-6 py-3', className)}>
      {items?.map((item, index) => (
        <div key={index} className="flex min-w-0 items-center gap-2 font-sans text-sm">
          <dt className="shrink-0 font-semibold text-tx-2">{item.label}</dt>
          <dd className="min-w-0 truncate text-tx-1">{item.value}</dd>
        </div>
      ))}
      {children}
    </dl>
  );
}
