import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { PageTitleRow } from '@/shell/PageTitleRow';

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
      data-account-section
      className={cn(
        'w-full min-w-0',
        width === 'form' && 'max-w-[960px]',
        width === 'page' && 'max-w-[1440px]',
        width === 'table' && 'max-w-[1920px]',
      )}
    >
      <header className="mb-6 min-h-[var(--page-title-row-h)]">
        <PageTitleRow
          title={title}
          description={subtitle}
          actions={actions}
          level={2}
          size="section"
        />
      </header>
      {children}
    </section>
  );
}
