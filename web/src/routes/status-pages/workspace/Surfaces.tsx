/** Status Pages workspace surface primitives. */
import type * as React from 'react';

import { cn } from '@/shell/lib/cn';

export const statusPageFlatTableClassName = 'rounded-none border-0 bg-transparent';
export const statusPageFlatStateClassName =
  'rounded-none border-0 bg-transparent shadow-none';

const SURFACE =
  'min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]';

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

/**
 * A list of a status page, with what it is showing at its head: the view, a
 * search, the filters, and the one thing that can be added to it.
 *
 * They are the head of the list they act on, not cards of their own above it:
 * a view switch in one card, a filter row in a second and the rows in a third
 * read as three things, and they are one.
 */
export function StatusPageList({
  head,
  action,
  children,
  className,
}: {
  /** The view and the filters, in the order they narrow the list. */
  head?: React.ReactNode;
  /** The list's own action, at the far end of the head. */
  action?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section data-status-page-list className={cn(SURFACE, className)}>
      {(head || action) && (
        <div
          data-status-page-list-head
          className="flex min-h-[48px] min-w-0 flex-wrap items-center gap-x-[12px] gap-y-[8px] border-b border-bd-0 px-[12px] py-[8px]"
        >
          <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-[12px] gap-y-[8px]">{head}</div>
          {action}
        </div>
      )}
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
    <section data-status-page-section className={cn(SURFACE, className)}>
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
