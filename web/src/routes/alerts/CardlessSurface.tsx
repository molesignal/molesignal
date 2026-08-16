import type { LucideIcon } from 'lucide-react';
import type * as React from 'react';

import { cn } from '@/shell/lib/cn';

export const alertFlatTableClassName = 'rounded-none border-0 bg-transparent';

export function AlertFilterTabs<T extends string>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (value: T) => void;
  options: Array<{ value: T; label: string; count: number }>;
}) {
  return (
    <div
      data-alert-filter-tabs
      className="flex max-w-full items-center gap-1 overflow-x-auto"
    >
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          onClick={() => onChange(option.value)}
          className={cn(
            'inline-flex h-10 shrink-0 items-center border-b-2 px-3 font-sans text-xs font-strong transition-colors duration-fast',
            value === option.value
              ? 'border-indigo text-tx-0'
              : 'border-transparent text-tx-2 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0',
          )}
        >
          {option.label}
          <span className="ml-1.5 font-mono text-type-micro text-tx-3">
            {option.count}
          </span>
        </button>
      ))}
    </div>
  );
}

export function AlertStateBand({
  icon: Icon,
  title,
  description,
  actions,
  tone = 'neutral',
  align = 'center',
}: {
  icon?: LucideIcon;
  title: React.ReactNode;
  description: React.ReactNode;
  actions?: React.ReactNode;
  tone?: 'neutral' | 'success';
  align?: 'start' | 'center';
}) {
  const centered = align === 'center';
  return (
    <section
      data-alert-state-band
      className={cn(
        'flex min-h-[160px] flex-col gap-4 bg-transparent px-4 py-6',
        centered ? 'items-center justify-center text-center' : 'items-start justify-center',
        tone === 'success'
          && 'min-h-[96px] bg-green-dim sm:flex-row sm:items-center sm:text-left',
      )}
    >
      {Icon && (
        <span
          className={cn(
            'grid h-10 w-10 shrink-0 place-items-center rounded-full',
            tone === 'success'
              ? 'bg-green/15 text-green-soft'
              : 'bg-bg-2 text-tx-3',
          )}
        >
          <Icon className="h-5 w-5" />
        </span>
      )}
      <div className={cn('min-w-0 flex-1', centered && tone === 'neutral' && 'flex-none')}>
        <div className="font-sans text-base font-display-strong text-tx-0">
          {title}
        </div>
        <p className="mt-1 max-w-xl font-sans text-sm leading-relaxed text-tx-2">
          {description}
        </p>
      </div>
      {actions && (
        <div className={cn('flex flex-wrap gap-2', tone === 'success' && 'sm:ml-auto')}>
          {actions}
        </div>
      )}
    </section>
  );
}
