import * as React from 'react';

import { cn } from '@/shell/lib/cn';

interface EmptyStateProps {
  title: React.ReactNode;
  description?: React.ReactNode;
  action?: React.ReactNode;
  /**
   * Mark as "awaiting backend" — used by pages whose listed backend
   * endpoint does not exist yet. Pages render this when the corresponding
   * `crates/api/src/http/routes/*.rs` route is not implemented; once it
   * lands the same component flips to the real list automatically.
   */
  awaitingBackend?: boolean;
  className?: string;
}

export function EmptyState({
  title,
  description,
  action,
  awaitingBackend = false,
  className,
}: EmptyStateProps) {
  return (
    <div
      className={cn(
        'flex min-h-48 flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-bd-1 bg-bg-1 px-8 py-10 text-center',
        className,
      )}
    >
      {awaitingBackend && (
        // Backend-pending == warning, not brand.
        <span className="rounded border border-yellow/30 bg-yellow-dim px-2 py-1 font-sans text-xs font-semibold tracking-normal text-yellow-soft">
          Awaiting backend
        </span>
      )}
      <div className="type-section-title font-sans font-semibold text-tx-0">{title}</div>
      {description && (
        <div className="max-w-md font-sans text-sm leading-relaxed text-tx-2">
          {description}
        </div>
      )}
      {action && <div className="mt-1">{action}</div>}
    </div>
  );
}
