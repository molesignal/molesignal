import * as React from 'react';

import { cn } from '@/shell/lib/cn';

type QueryToolbarTone = 'indigo' | 'orange';

const activeToneClass: Record<QueryToolbarTone, string> = {
  indigo: 'bg-indigo text-white shadow-sm',
  orange: 'bg-orange text-[var(--orange-fg)] shadow-sm',
};

interface QueryWorkbenchProps {
  toolbar: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
  footer?: React.ReactNode;
  appearance?: 'default' | 'surface';
}

export function QueryWorkbench({
  toolbar,
  children,
  className,
  bodyClassName,
  footer,
  appearance = 'default',
}: QueryWorkbenchProps) {
  const surface = appearance === 'surface';

  return (
    <section
      data-query-workbench-appearance={appearance}
      className={cn(
        surface
          ? 'mx-[20px] mb-[12px] rounded-md bg-[var(--functional-surface)] px-[12px] py-[8px] [box-shadow:var(--shadow-functional-surface)]'
          : 'border-b border-bd-0 bg-bg-1 p-3',
        className,
      )}
    >
      <div
        className={cn(
          'flex w-full flex-wrap items-center overflow-x-auto xl:flex-nowrap',
          surface
            ? 'min-h-10 gap-[8px] rounded-md bg-transparent'
            : 'min-h-11 gap-2 rounded-lg border border-bd-1 bg-bg-2/80 px-2 py-1.5 shadow-sm',
        )}
      >
        {toolbar}
      </div>
      <div className={cn(surface ? 'mt-[8px]' : 'mt-3', bodyClassName)}>{children}</div>
      {footer ? (
        <div
          className={cn(
            'mt-3 rounded-lg px-3 py-2',
            surface ? 'bg-[var(--control-surface)]' : 'border border-bd-1 bg-bg-1',
          )}
        >
          {footer}
        </div>
      ) : null}
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
  flat?: boolean;
}

export const QueryToolbarButton = React.forwardRef<
  HTMLButtonElement,
  QueryToolbarButtonProps
>(function QueryToolbarButton({
  active = false,
  tone = 'indigo',
  flat = false,
  className,
  children,
  type = 'button',
  ...props
}, ref) {
  return (
    <button
      {...props}
      ref={ref}
      type={type}
      className={cn(
        'inline-flex h-8 shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded px-3 font-sans text-xs font-strong text-tx-2 transition-colors hover:bg-bg-3 hover:text-tx-0 disabled:cursor-not-allowed disabled:opacity-50',
        active && activeToneClass[tone],
        flat && 'shadow-none',
        className,
      )}
    >
      {children}
    </button>
  );
});

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
  flat?: boolean;
  selectionStyle?: 'fill' | 'underline';
}

export function QueryToolbarTabs<T extends string>({
  tabs,
  activeId,
  onChange,
  tone = 'indigo',
  flat = false,
  selectionStyle = 'fill',
}: QueryToolbarTabsProps<T>) {
  const underline = selectionStyle === 'underline';

  return (
    <QueryToolbarGroup
      role="tablist"
      data-query-tab-selection={selectionStyle}
      className={cn(
        'shrink-0',
        underline
          ? 'h-10 items-stretch gap-0 rounded-none border-0 bg-transparent p-0'
          : flat && 'border-0 bg-[var(--control-surface)]',
      )}
    >
      {tabs.map((tab) => {
        const active = tab.id === activeId;
        return (
          <QueryToolbarButton
            key={tab.id}
            role="tab"
            aria-selected={active}
            active={!underline && active}
            tone={tone}
            flat={flat || underline}
            onClick={() => onChange(tab.id)}
            className={cn(
              'min-w-[72px]',
              underline &&
                'relative h-10 rounded-none bg-transparent px-3 shadow-none after:absolute after:inset-x-3 after:bottom-0 after:h-[3px] after:rounded-t-md after:bg-transparent hover:bg-transparent hover:text-tx-0',
              underline && active && 'text-indigo-soft after:bg-indigo',
            )}
          >
            <span className="truncate">{tab.label}</span>
            {tab.count !== undefined ? (
              <span className="grid min-w-5 place-items-center rounded-full bg-bg-3 px-1.5 text-xs text-tx-2">
                {tab.count}
              </span>
            ) : null}
          </QueryToolbarButton>
        );
      })}
    </QueryToolbarGroup>
  );
}
