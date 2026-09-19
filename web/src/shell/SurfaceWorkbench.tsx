import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { PageBody, PageHeader } from '@/shell/PageHeader';

export const surfacePageRootClass = 'min-h-0 bg-[var(--page-canvas)]';

export const surfacePanelClass =
  'rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]';

export const surfaceModuleNavigationClass =
  'mx-[20px] mb-[12px] overflow-hidden rounded-lg border-0 bg-[var(--functional-surface)] px-[12px] [box-shadow:var(--shadow-functional-surface)]';

export const surfaceModuleNavigationRowClass =
  'flex min-h-11 items-stretch gap-5 overflow-x-auto';

export const surfaceModuleNavigationItemClass =
  "relative inline-flex h-11 shrink-0 items-center rounded-md border-0 px-1 font-sans text-sm font-strong text-tx-2 outline-none transition-colors duration-fast after:pointer-events-none after:absolute after:inset-x-0 after:bottom-0 after:h-[3px] after:bg-transparent after:content-[''] hover:bg-[var(--control-surface)] hover:text-tx-0 focus-visible:bg-[var(--control-surface)] focus-visible:text-tx-0";

export const surfaceModuleNavigationActiveClass =
  'bg-transparent text-tx-0 after:bg-indigo';

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
