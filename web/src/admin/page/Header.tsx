import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { PageTitleRow } from '@/shell/PageTitleRow';

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
      data-admin-page-header
      className={cn(
        'min-h-12 bg-transparent px-5 py-1.5',
        className,
      )}
    >
      <PageTitleRow
        title={title}
        description={subtitle}
        actions={actions}
        level={2}
        size="section"
      />
    </div>
  );
}
