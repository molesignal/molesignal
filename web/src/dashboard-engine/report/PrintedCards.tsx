import type * as React from 'react';

import { cn } from '@/shell/lib/cn';

const HEADING = 'min-w-0 truncate font-sans text-xs font-semibold text-tx-1';

/**
 * A panel as a report prints it: its title and what it draws.
 *
 * On screen the header also carries the panel's menu, its description on hover,
 * a progress mark and the visualization's name. None of that says anything on
 * paper, so none of it is drawn.
 */
export function PrintedPanel({
  title,
  transparent = false,
  children,
}: {
  title: string;
  transparent?: boolean | undefined;
  children: React.ReactNode;
}) {
  return (
    <section
      data-printed-panel
      aria-label={title}
      className={cn(
        'flex h-full min-h-0 flex-col overflow-hidden border border-bd-0 bg-bg-1',
        transparent ? 'border-transparent bg-transparent' : 'rounded-md',
      )}
    >
      <header className="flex h-9 shrink-0 items-center border-b border-bd-0 px-2.5">
        <h3 className={HEADING}>{title}</h3>
      </header>
      <div className="min-h-0 flex-1 p-2">{children}</div>
    </section>
  );
}

/** The heading of a row or group on paper: its name, with no control to fold it. */
export function PrintedSectionHeading({ children }: { children: React.ReactNode }) {
  return (
    <header className="flex h-9 shrink-0 items-center border-b border-bd-0 px-3">
      <h3 className={HEADING}>{children}</h3>
    </header>
  );
}
