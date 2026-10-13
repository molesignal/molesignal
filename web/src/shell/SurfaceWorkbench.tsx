import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { PageBody, PageHeader, useRouteModuleIcon } from '@/shell/PageHeader';

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

/**
 * A page's name set into the first row of its workbench.
 *
 * The explorers (Logs, Traces) give every row they can to the data, so the name
 * does not get a row to itself. It keeps the icon, face and size of a page title
 * and sits where a page title would start, so moving between an explorer and any
 * other page does not move the name; a hairline parts it from the controls.
 */
export function WorkbenchTitle({ title }: { title: React.ReactNode }) {
  const ModuleIcon = useRouteModuleIcon();

  return (
    <div data-workbench-title className="flex shrink-0 items-center gap-2">
      {ModuleIcon && (
        <span
          aria-hidden
          data-testid="page-header-module-icon"
          className="grid h-7 w-7 shrink-0 place-items-center rounded-md bg-indigo/10 text-indigo"
        >
          <ModuleIcon className="h-3.5 w-3.5" strokeWidth={1.8} />
        </span>
      )}
      <h1 className="type-page-title whitespace-nowrap font-sans font-display-strong tracking-normal text-tx-0">
        {title}
      </h1>
      <span aria-hidden className="ml-1 h-[20px] w-px shrink-0 bg-bd-1" />
    </div>
  );
}
