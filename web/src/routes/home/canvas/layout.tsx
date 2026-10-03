import { ChevronRight } from 'lucide-react';
import type * as React from 'react';

import { cn } from '@/shell/lib/cn';

import './canvas.css';

/** Vertical rhythm for Home: bands are separated by `--home-band-gap` (see canvas.css). */
export function OperationsCanvas({ children }: { children: React.ReactNode }) {
  return (
    <div
      className="home-operations-canvas mx-auto w-full max-w-[2200px] bg-[var(--page-canvas)]"
      data-testid="home-operations-canvas"
    >
      {children}
    </div>
  );
}

export function CanvasSurfaceGrid({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div data-home-surface-grid className={cn('home-canvas-surface-grid', className)}>
      {children}
    </div>
  );
}

/** A stack of cards that keeps its own height instead of stretching to a grid row. */
export function CanvasColumn({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return <div className={cn('home-canvas-column', className)}>{children}</div>;
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
  riseIndex,
}: {
  title: React.ReactNode;
  icon?: React.ReactNode;
  actions?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
  ariaLabel?: string;
  testId?: string;
  /** Position in the entrance sequence; omit to render without an entrance. */
  riseIndex?: number;
}) {
  return (
    <section
      aria-label={ariaLabel}
      className={cn(
        'flex min-h-0 min-w-0 flex-col overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        riseIndex !== undefined && 'home-rise',
        className,
      )}
      style={riseIndex !== undefined ? ({ '--rise-index': riseIndex } as React.CSSProperties) : undefined}
      data-home-surface="section"
      data-testid={testId}
    >
      <div className="home-canvas-section-header type-section-title flex min-h-[48px] shrink-0 items-center gap-3 px-[20px] py-2 font-sans font-strong text-tx-0">
        <h2 className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden text-ellipsis whitespace-nowrap">
          {icon}
          {title}
        </h2>
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
      className="-mr-1 inline-flex h-[32px] shrink-0 items-center gap-1 px-1 font-sans text-xs font-strong text-tx-2 transition-colors duration-fast hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 focus-visible:outline-none"
    >
      {label}
      <ChevronRight aria-hidden="true" className="h-3 w-3" />
    </button>
  );
}
