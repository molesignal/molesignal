import * as React from 'react';

import { cn } from '@/shell/lib/cn';

export function AccountSection({
  title,
  subtitle,
  actions,
  width = 'form',
  children,
}: {
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  actions?: React.ReactNode;
  width?: 'form' | 'page' | 'table';
  children: React.ReactNode;
}) {
  return (
    <section
      className={cn(
        'min-w-0',
        width === 'form' && 'max-w-[760px]',
        width === 'page' && 'max-w-[1080px]',
        width === 'table' && 'max-w-[1280px]',
      )}
    >
      <header className="mb-5 flex min-h-12 items-start gap-4">
        <div className="min-w-0 flex-1">
          <h2 className="type-section-title font-sans font-display-strong text-tx-0">
            {title}
          </h2>
          {subtitle && (
            <p className="mt-1 max-w-2xl font-sans text-xs leading-relaxed text-tx-2">
              {subtitle}
            </p>
          )}
        </div>
        {actions && (
          <div className="flex shrink-0 items-center gap-2">{actions}</div>
        )}
      </header>
      {children}
    </section>
  );
}
