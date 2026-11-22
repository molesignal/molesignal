import { ChevronRight } from 'lucide-react';
import type * as React from 'react';
import { Link } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';

/**
 * A panel of Home: a titled area with a rule under its head, the way a dashboard sets its
 * panels, so the page reads as one board rather than a stack of cards.
 */
export function Panel({
  title,
  count,
  action,
  children,
  className,
  bodyClassName,
  testId,
}: {
  title: React.ReactNode;
  /** A figure said after the title, quieter: how many the panel holds. */
  count?: number | undefined;
  /** Where the panel's whole list opens. */
  action?: { label: string; to: string } | undefined;
  children: React.ReactNode;
  className?: string | undefined;
  bodyClassName?: string | undefined;
  testId?: string | undefined;
}) {
  return (
    <section
      data-home-panel
      data-testid={testId}
      className={cn(
        'flex min-h-0 min-w-0 flex-col overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      <header className="flex h-[40px] shrink-0 items-center gap-2 border-b border-[var(--row-rule)] px-[12px]">
        <h2 className="min-w-0 truncate font-sans text-md font-display text-tx-0">{title}</h2>
        {count !== undefined && <span className="font-sans text-sm tabular-nums text-tx-3">{count}</span>}
        {action && (
          <Link
            to={action.to}
            className="-mr-1 ml-auto inline-flex h-[28px] shrink-0 items-center gap-0.5 rounded-sm px-1 font-sans text-xs text-tx-2 transition-colors duration-fast hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0"
          >
            {action.label}
            <ChevronRight aria-hidden="true" className="h-3 w-3" />
          </Link>
        )}
      </header>
      <div className={cn('flex min-h-0 flex-1 flex-col', bodyClassName)}>{children}</div>
    </section>
  );
}

/** What a panel says when it has nothing to list: one quiet line, centred in its room. */
export function PanelNote({ children, testId }: { children: React.ReactNode; testId?: string }) {
  return (
    <p
      data-testid={testId}
      className="flex min-h-[72px] flex-1 items-center justify-center px-[12px] py-4 text-center font-sans text-sm text-tx-3"
    >
      {/* One run of text, so a link inside keeps the space before it. */}
      <span>{children}</span>
    </p>
  );
}
