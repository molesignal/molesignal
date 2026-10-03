import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { PageBody, PageHeader } from '@/shell/PageHeader';

export const surfacePageRootClass = 'min-h-0 bg-[var(--page-canvas)]';

export const surfacePanelClass =
  'rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]';

export function SurfacePageHeader({
  className,
  ...props
}: React.ComponentProps<typeof PageHeader>) {
  return (
    <PageHeader
      {...props}
      className={cn(
        'my-[12px] shrink-0 border-b-0 bg-[var(--page-canvas)] px-[20px]',
        className,
      )}
    />
  );
}

export function SurfacePageBody({
  className,
  ...props
}: React.ComponentProps<typeof PageBody>) {
  return (
    <PageBody
      {...props}
      padded={false}
      className={cn(
        'bg-[var(--page-canvas)] px-[20px] pb-[20px]',
        className,
      )}
    />
  );
}
