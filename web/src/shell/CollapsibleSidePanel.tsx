import { ChevronLeft, ChevronRight } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

const SIDE_PANEL_DEFAULT_MAX_WIDTH = 480;
const SIDE_PANEL_RESIZE_STEP = 16;

function clampPanelWidth(width: number, minWidth: number, maxWidth: number, fallback: number): number {
  if (!Number.isFinite(width)) return fallback;
  return Math.min(maxWidth, Math.max(minWidth, Math.round(width)));
}

interface CollapsibleSidePanelProps {
  title: React.ReactNode;
  collapsed: boolean;
  onCollapsedChange: (collapsed: boolean) => void;
  children: React.ReactNode;
  variant?: 'default' | 'utility';
  className?: string;
  widthClassName?: string;
  bodyClassName?: string;
  footer?: React.ReactNode;
  collapseLabel?: string;
  expandLabel?: string;
  resizable?: boolean;
  defaultWidth?: number;
  minWidth?: number;
  maxWidth?: number;
  resizeLabel?: string;
}

export function CollapsibleSidePanel({
  title,
  collapsed,
  onCollapsedChange,
  children,
  variant = 'default',
  className,
  widthClassName = 'w-[280px]',
  bodyClassName,
  footer,
  collapseLabel = 'Collapse panel',
  expandLabel = 'Expand panel',
  resizable = false,
  defaultWidth = 280,
  minWidth,
  maxWidth = SIDE_PANEL_DEFAULT_MAX_WIDTH,
  resizeLabel = 'Resize panel',
}: CollapsibleSidePanelProps) {
  const utility = variant === 'utility';
  const requestedMinWidth = minWidth ?? defaultWidth;
  const resolvedMinWidth = Math.min(requestedMinWidth, maxWidth);
  const resolvedMaxWidth = Math.max(requestedMinWidth, maxWidth);
  const initialWidth = clampPanelWidth(
    defaultWidth,
    resolvedMinWidth,
    resolvedMaxWidth,
    resolvedMinWidth,
  );
  const [panelWidth, setPanelWidth] = React.useState(initialWidth);
  const [resizing, setResizing] = React.useState(false);
  const panelWidthRef = React.useRef(panelWidth);
  const resizeCleanupRef = React.useRef<(() => void) | null>(null);

  const applyPanelWidth = React.useCallback((width: number) => {
    const nextWidth = clampPanelWidth(
      width,
      resolvedMinWidth,
      resolvedMaxWidth,
      initialWidth,
    );
    panelWidthRef.current = nextWidth;
    setPanelWidth(nextWidth);
  }, [initialWidth, resolvedMaxWidth, resolvedMinWidth]);

  React.useEffect(() => {
    applyPanelWidth(panelWidthRef.current);
  }, [applyPanelWidth]);

  React.useEffect(() => () => {
    resizeCleanupRef.current?.();
  }, []);

  React.useEffect(() => {
    if (!collapsed) return;
    resizeCleanupRef.current?.();
    setResizing(false);
  }, [collapsed]);

  const beginResize = React.useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (!resizable || event.button !== 0) return;
    event.preventDefault();
    resizeCleanupRef.current?.();

    const pointerId = event.pointerId;
    const startClientX = event.clientX;
    const startWidth = panelWidthRef.current;
    const previousCursor = document.body.style.cursor;
    const previousUserSelect = document.body.style.userSelect;

    function cleanup() {
      globalThis.removeEventListener('pointermove', update);
      globalThis.removeEventListener('pointerup', finish);
      globalThis.removeEventListener('pointercancel', finish);
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousUserSelect;
      resizeCleanupRef.current = null;
    }

    function update(pointerEvent: PointerEvent) {
      if (pointerEvent.pointerId !== pointerId) return;
      pointerEvent.preventDefault();
      applyPanelWidth(startWidth + pointerEvent.clientX - startClientX);
    }

    function finish(pointerEvent: PointerEvent) {
      if (pointerEvent.pointerId !== pointerId) return;
      cleanup();
      setResizing(false);
    }

    resizeCleanupRef.current = cleanup;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    setResizing(true);
    globalThis.addEventListener('pointermove', update, { passive: false });
    globalThis.addEventListener('pointerup', finish);
    globalThis.addEventListener('pointercancel', finish);
    try {
      event.currentTarget.setPointerCapture(pointerId);
    } catch {
      // Window-level pointer tracking keeps the resize active outside the handle.
    }
  }, [applyPanelWidth, resizable]);

  const resizeFromKeyboard = React.useCallback((event: React.KeyboardEvent<HTMLDivElement>) => {
    let nextWidth: number | null = null;
    const step = SIDE_PANEL_RESIZE_STEP * (event.shiftKey ? 2 : 1);
    if (event.key === 'ArrowLeft') nextWidth = panelWidthRef.current - step;
    if (event.key === 'ArrowRight') nextWidth = panelWidthRef.current + step;
    if (event.key === 'Home') nextWidth = resolvedMinWidth;
    if (event.key === 'End') nextWidth = resolvedMaxWidth;
    if (nextWidth === null) return;
    event.preventDefault();
    applyPanelWidth(nextWidth);
  }, [applyPanelWidth, resolvedMaxWidth, resolvedMinWidth]);

  if (collapsed) {
    return (
      <aside
        data-variant={variant}
        className={cn(
          'flex w-11 shrink-0 flex-col items-center overflow-hidden border-r border-bd-0',
          utility ? 'bg-bg-2' : 'bg-bg-1',
          className,
        )}
      >
        <button
          type="button"
          onClick={() => onCollapsedChange(false)}
          aria-label={expandLabel}
          title={expandLabel}
          className={cn(
            'mt-2 grid h-8 w-8 place-items-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0',
            !utility && 'border border-bd-1 bg-bg-2',
          )}
        >
          <ChevronRight className="h-3.5 w-3.5" />
        </button>
        <div
          className={cn(
            'mt-4 max-h-[180px] rotate-180 overflow-hidden text-ellipsis whitespace-nowrap font-sans text-xs font-strong tracking-normal text-tx-3 [writing-mode:vertical-rl]',
            !utility && 'uppercase',
          )}
        >
          {title}
        </div>
      </aside>
    );
  }

  return (
    <aside
      data-variant={variant}
      style={resizable
        ? ({ '--side-panel-width': `${panelWidth}px` } as React.CSSProperties)
        : undefined}
      className={cn(
        'flex shrink-0 flex-col border-r border-bd-0',
        utility ? 'bg-bg-2' : 'bg-bg-1',
        widthClassName,
        resizable && 'relative lg:w-[var(--side-panel-width)]',
        className,
      )}
    >
      <div
        className={cn(
          'flex shrink-0 items-center gap-2 font-sans text-xs font-strong',
          utility
            ? 'h-10 px-3 text-tx-1'
            : 'h-11 border-b border-bd-0 px-3.5 uppercase tracking-wide text-tx-2',
        )}
      >
        <span className="min-w-0 flex-1 truncate">{title}</span>
        <button
          type="button"
          onClick={() => onCollapsedChange(true)}
          aria-label={collapseLabel}
          title={collapseLabel}
          className="grid h-8 w-8 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0"
        >
          <ChevronLeft className="h-3.5 w-3.5" />
        </button>
      </div>
      <div className={cn('min-h-0 flex-1 overflow-hidden', bodyClassName)}>{children}</div>
      {footer ? <div className="shrink-0">{footer}</div> : null}
      {resizable ? (
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label={resizeLabel}
          aria-valuemin={resolvedMinWidth}
          aria-valuemax={resolvedMaxWidth}
          aria-valuenow={panelWidth}
          aria-valuetext={`${panelWidth}px`}
          tabIndex={0}
          title={resizeLabel}
          data-resizing={resizing || undefined}
          onPointerDown={beginResize}
          onKeyDown={resizeFromKeyboard}
          onDoubleClick={() => applyPanelWidth(initialWidth)}
          className="group absolute inset-y-0 -right-1 z-20 hidden w-2 touch-none select-none cursor-col-resize focus-visible:outline-none lg:block"
        >
          <span
            aria-hidden="true"
            className={cn(
              'absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-transparent transition-colors duration-fast',
              'group-hover:bg-indigo group-focus-visible:bg-indigo',
              resizing && 'bg-indigo',
            )}
          />
        </div>
      ) : null}
    </aside>
  );
}

interface SidePanelSectionProps {
  title: React.ReactNode;
  count?: number | undefined;
  children: React.ReactNode;
  className?: string | undefined;
}

export function SidePanelSection({ title, count, children, className }: SidePanelSectionProps) {
  return (
    <section className={cn('py-1.5', className)}>
      <div className="flex h-7 items-center gap-2 px-2 font-sans text-xs font-strong text-tx-3">
        <span className="min-w-0 flex-1 truncate">{title}</span>
        {count !== undefined ? <span className="font-mono text-xs font-normal">{count}</span> : null}
      </div>
      <div className="[&>*:last-child]:border-b-0">{children}</div>
    </section>
  );
}
