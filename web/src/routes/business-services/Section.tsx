import * as React from 'react';

import { cn } from '@/shell/lib/cn';

/** One card of a business service page, named by its own heading. */
export function ServiceSection({
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
  const headingId = React.useId();

  return (
    <section
      aria-labelledby={headingId}
      className={cn(
        'min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      <div className="flex min-h-12 items-center justify-between gap-4 px-4 py-2.5">
        <div className="min-w-0">
          <h2 id={headingId} className="font-sans text-sm font-display-strong text-tx-0">
            {title}
          </h2>
          {description && <p className="mt-0.5 text-pretty font-sans text-xs text-tx-2">{description}</p>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}
