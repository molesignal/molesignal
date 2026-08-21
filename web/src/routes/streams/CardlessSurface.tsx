import type * as React from 'react';

import { uiLabelClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { Switch } from '@/shell/ui/switch';

export interface StreamKpiItem {
  label: React.ReactNode;
  value: React.ReactNode;
  note?: React.ReactNode;
  tone?: 'neutral' | 'good' | 'warn' | 'danger';
}

export const streamFlatTableClassName =
  'overflow-x-auto rounded-none border-0 bg-transparent';

export function StreamKpiBand({
  items,
  className,
}: {
  items: readonly StreamKpiItem[];
  className?: string;
}) {
  if (items.length === 0) return null;

  return (
    <section
      data-stream-kpis
      className={cn(
        'grid grid-cols-1 gap-[12px] sm:grid-cols-2 xl:grid-cols-5',
        className,
      )}
    >
      {items.map((item, index) => (
        <div key={index} className="min-h-[92px] min-w-0 rounded-md bg-[var(--functional-surface)] px-3 py-3 [box-shadow:var(--shadow-functional-surface)]">
          <div className={uiLabelClass}>{item.label}</div>
          <div
            className={cn(
              'mt-2 truncate font-sans text-2xl font-display-strong leading-none tracking-[-0.025em] tabular-nums',
              (!item.tone || item.tone === 'neutral') && 'text-tx-0',
              item.tone === 'good' && 'text-green',
              item.tone === 'warn' && 'text-yellow',
              item.tone === 'danger' && 'text-red',
            )}
          >
            {item.value}
          </div>
          {item.note && (
            <div className="mt-1.5 truncate font-sans text-xs text-tx-2">
              {item.note}
            </div>
          )}
        </div>
      ))}
    </section>
  );
}

export function StreamSection({
  title,
  description,
  actions,
  children,
  className,
  bodyClassName,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  actions?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <section
      data-stream-section
      className={cn(
        'min-w-0 rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      <header className="flex min-h-[50px] items-center gap-4 py-2.5">
        <div className="min-w-0 flex-1">
          <h2 className="truncate font-sans text-sm font-display-strong text-tx-0">
            {title}
          </h2>
          {description && (
            <p className="mt-0.5 truncate font-sans text-xs text-tx-2">
              {description}
            </p>
          )}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-1.5">{actions}</div>}
      </header>
      <div className={cn('pt-4', bodyClassName)}>{children}</div>
    </section>
  );
}

export function StreamSettingsSection({
  title,
  description,
  children,
}: {
  title: React.ReactNode;
  description: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section data-stream-settings-section className="min-w-0 rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)]">
      <header className="min-h-[68px] py-3">
        <h2 className="font-sans text-sm font-display-strong text-tx-0">{title}</h2>
        <p className="mt-1 font-sans text-xs leading-relaxed text-tx-2">{description}</p>
      </header>
      <div className="space-y-4 pt-4">{children}</div>
    </section>
  );
}

export function StreamToggleRow({
  title,
  hint,
  checked,
  onChange,
}: {
  title: React.ReactNode;
  hint?: React.ReactNode;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex min-h-12 items-start gap-4 py-2">
      <div className="min-w-0">
        <div className="font-sans text-xs font-semibold text-tx-0">{title}</div>
        {hint && (
          <div className="mt-1 font-sans text-xs leading-relaxed text-tx-3">{hint}</div>
        )}
      </div>
      <Switch
        checked={checked}
        onCheckedChange={onChange}
        className="ml-auto shrink-0 data-[state=checked]:bg-indigo"
      />
    </div>
  );
}
