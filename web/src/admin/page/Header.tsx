import * as React from 'react';

import { cn } from '@/shell/lib/cn';

interface AdminPageHeaderProps {
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
}

/**
 * Section header used inside Settings, IAM and compact admin sub-pages.
 * It deliberately sits one level below the 22px shell/PageHeader masthead:
 * 15px section title, 12.5px supporting copy and full-size actions.
 */
export function PageHeader({ title, subtitle, actions, className }: AdminPageHeaderProps) {
  return (
    <div
      className={cn(
        'flex min-h-14 items-center gap-4 border-b border-bd-0 bg-bg-1 px-5 py-3.5',
        className,
      )}
    >
      <div className="flex min-w-0 flex-col gap-0.5">
        <div className="type-section-title font-sans font-display-strong text-tx-0">{title}</div>
        {subtitle && (
          <span className="type-label truncate text-tx-2">{subtitle}</span>
        )}
      </div>
      {actions && <div className="ml-auto flex items-center gap-2">{actions}</div>}
    </div>
  );
}
