import * as React from 'react';

import { cn } from '@/shell/lib/cn';

type QueryToolbarTone = 'blue' | 'indigo' | 'orange';

const activeToneClass: Record<QueryToolbarTone, string> = {
  blue: 'bg-blue text-white shadow-sm',
  indigo: 'bg-indigo text-white shadow-sm',
  orange: 'bg-orange text-white shadow-sm',
};

interface QueryWorkbenchProps {
  toolbar: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
  footer?: React.ReactNode;
}

export function QueryWorkbench({ toolbar, children, className, bodyClassName, footer }: QueryWorkbenchProps) {
  return (
    <section className={cn('border-b border-bd-0 bg-bg-1 p-3', className)}>
      <div className="flex min-h-11 w-full flex-wrap items-center gap-2 overflow-x-auto rounded-lg border border-bd-1 bg-bg-2/80 px-2 py-1.5 shadow-sm xl:flex-nowrap">
        {toolbar}
      </div>
      <div className={cn('mt-3', bodyClassName)}>{children}</div>
      {footer ? <div className="mt-3 rounded-lg border border-bd-1 bg-bg-1 px-3 py-2">{footer}</div> : null}
    </section>
  );
}

interface QueryToolbarGroupProps extends React.HTMLAttributes<HTMLDivElement> {
  children: React.ReactNode;
}

export function QueryToolbarGroup({ children, className, ...props }: QueryToolbarGroupProps) {
  return (
    <div
      {...props}
      className={cn('flex h-9 shrink-0 items-center gap-0.5 rounded-md border border-bd-0 bg-bg-1 p-0.5', className)}
    >
      {children}
    </div>
  );
}

interface QueryToolbarButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  active?: boolean;
  tone?: QueryToolbarTone;
}

export function QueryToolbarButton({
  active = false,
  tone = 'blue',
  className,
  children,
  type = 'button',
  ...props
}: QueryToolbarButtonProps) {
  return (
    <button
      {...props}
      type={type}
      className={cn(
        'inline-flex h-8 shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded px-3 font-sans text-xs font-strong text-tx-2 transition-colors hover:bg-bg-3 hover:text-tx-0 disabled:cursor-not-allowed disabled:opacity-50',
        active && activeToneClass[tone],
        className,
      )}
    >
      {children}
    </button>
  );
}

export interface QueryToolbarTab<T extends string> {
  id: T;
  label: React.ReactNode;
  count?: number | string | undefined;
}

interface QueryToolbarTabsProps<T extends string> {
  tabs: Array<QueryToolbarTab<T>>;
  activeId: T;
  onChange: (id: T) => void;
  tone?: QueryToolbarTone;
}

export function QueryToolbarTabs<T extends string>({
  tabs,
  activeId,
  onChange,
  tone = 'blue',
}: QueryToolbarTabsProps<T>) {
  return (
    <QueryToolbarGroup className="shrink-0">
      {tabs.map((tab) => (
        <QueryToolbarButton
          key={tab.id}
          active={tab.id === activeId}
          tone={tone}
          onClick={() => onChange(tab.id)}
          className="min-w-[72px]"
        >
          <span className="truncate">{tab.label}</span>
          {tab.count !== undefined ? (
            <span className="grid min-w-5 place-items-center rounded-full bg-bg-3 px-1.5 text-xs text-tx-2">
              {tab.count}
            </span>
          ) : null}
        </QueryToolbarButton>
      ))}
    </QueryToolbarGroup>
  );
}
