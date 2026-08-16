import { ChevronRight } from 'lucide-react';
import type * as React from 'react';

import type * as homeApi from '@/api/home';
import { uiLabelClass, uiLabelStrongClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';

import './canvas.css';

const STATUS_ICON_CLASS: Partial<Record<homeApi.HomeHealthStatus, string>> = {
  healthy: 'bg-green-dim text-green-soft',
  degraded: 'bg-red-dim text-red-soft',
  delayed: 'bg-yellow-dim text-yellow-soft',
};

export function OperationsCanvas({ children }: { children: React.ReactNode }) {
  return (
    <div
      className="home-operations-canvas mx-auto w-full max-w-[2200px] overflow-hidden bg-bg-0"
      data-testid="home-operations-canvas"
    >
      {children}
    </div>
  );
}

export function CanvasDividerGrid({
  children,
  className,
  topDivider = false,
}: {
  children: React.ReactNode;
  className?: string;
  topDivider?: boolean;
}) {
  return (
    <div
      className={cn(
        'home-canvas-divider-grid grid',
        topDivider && 'border-t border-bd-0',
        className,
      )}
    >
      {children}
    </div>
  );
}

export function CanvasKpiStrip({ children, label }: { children: React.ReactNode; label: string }) {
  return (
    <section
      aria-label={label}
      className="home-canvas-kpi-grid grid border-b border-bd-0 bg-bg-0"
    >
      {children}
    </section>
  );
}

export function CanvasKpi({
  label,
  value,
  detail,
  icon,
  status,
  onClick,
}: {
  label: string;
  value: React.ReactNode;
  detail: React.ReactNode;
  icon: React.ReactNode;
  status?: homeApi.HomeHealthStatus | undefined;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="home-canvas-kpi group relative min-w-0 bg-bg-0 px-3 py-3 text-left transition-colors duration-fast hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:text-tx-0 focus-visible:outline-none"
    >
      <div className="flex min-w-0 items-center gap-1.5">
        <span
          className={cn(
            'grid h-4 w-4 shrink-0 place-items-center rounded bg-bg-2 text-tx-3 transition-colors group-hover:bg-bg-3 group-hover:text-tx-1 [&>svg]:h-3 [&>svg]:w-3',
            status && STATUS_ICON_CLASS[status],
          )}
        >
          {icon}
        </span>
        <span className={cn(uiLabelClass, 'min-w-0 flex-1 truncate')}>{label}</span>
      </div>
      <div className="mt-2.5 truncate font-sans text-2xl font-display-strong leading-none tracking-[-0.025em] text-tx-0">
        {value}
      </div>
      <div className="mt-1.5 line-clamp-2 font-sans text-xs leading-snug text-tx-2">
        {detail}
      </div>
      <ChevronRight
        aria-hidden="true"
        className="absolute bottom-3 right-3 h-3.5 w-3.5 translate-x-1 text-tx-3 opacity-0 transition-[opacity,transform] duration-fast group-hover:translate-x-0 group-hover:opacity-100 group-focus-visible:translate-x-0 group-focus-visible:opacity-100"
      />
    </button>
  );
}

export function CanvasSection({
  title,
  icon,
  actions,
  children,
  className,
  bodyClassName,
  ariaLabel,
  testId,
}: {
  title: React.ReactNode;
  icon?: React.ReactNode;
  actions?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
  ariaLabel?: string;
  testId?: string;
}) {
  return (
    <section
      aria-label={ariaLabel}
      className={cn(
        'flex min-h-0 min-w-0 flex-col overflow-hidden bg-bg-0',
        className,
      )}
      data-testid={testId}
    >
      <div
        className={cn(
          'home-canvas-section-header flex min-h-10 shrink-0 items-center gap-3 px-4 py-2',
          uiLabelStrongClass,
        )}
      >
        <div className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden text-ellipsis whitespace-nowrap">
          {icon}
          {title}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-1.5">{actions}</div>}
      </div>
      <div className={cn('flex min-h-0 flex-1 flex-col', bodyClassName)}>{children}</div>
    </section>
  );
}

export function CanvasHeaderAction({
  label,
  onClick,
}: {
  label: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="-mr-1 inline-flex h-8 shrink-0 items-center gap-1 px-1 font-sans text-xs font-strong text-tx-2 transition-colors duration-fast hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 focus-visible:outline-none"
    >
      {label}
      <ChevronRight aria-hidden="true" className="h-3 w-3" />
    </button>
  );
}
