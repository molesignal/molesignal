import { ArrowLeft, ChevronDown, ChevronUp, Maximize2, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';
import { ResizeHandle } from '@/shell/ResizeHandle';
import { Button } from '@/shell/ui/button';
import { Sheet, SheetClose, SheetContent, SheetTitle } from '@/shell/ui/sheet';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';
import { TRACE_LAYOUT_LIMITS, useTraceLayout } from '@/viz/trace/view/layout';
import { TraceView } from '@/viz/trace/view/TraceView';

import { currentTrace, type DrawerTrace, type TraceDrawerState } from './model';

/** The neighbours of the trace in the list it was opened from. */
export interface DrawerNeighbours {
  onPrevious?: (() => void) | undefined;
  onNext?: (() => void) | undefined;
}

/** The room the list keeps beside the drawer at the least. */
const LIST_ROOM = 48;

function useWindowWidth(): number {
  const [width, setWidth] = React.useState(() => (typeof window === 'undefined' ? 1440 : window.innerWidth));

  React.useEffect(() => {
    const measure = () => setWidth(window.innerWidth);
    window.addEventListener('resize', measure);
    return () => window.removeEventListener('resize', measure);
  }, []);

  return width;
}

/**
 * A trace opened from the results without leaving them: its whole view in a
 * drawer most of the window wide, whose left edge the reader drags. The keys
 * J and K, or the arrows in its head, step to the next and the previous trace
 * of the list; closing it gives the focus back to what opened it.
 */
export function TraceDetailDrawer({
  state,
  onClose,
  onFollow,
  onBack,
  neighbours,
  pageHref,
  returnFocusRef,
  trail,
}: {
  state: TraceDrawerState | null;
  onClose: () => void;
  /** A link from the trace was followed into another one. */
  onFollow: (trace: DrawerTrace) => void;
  onBack: () => void;
  neighbours?: DrawerNeighbours | undefined;
  /** The trace's own page. */
  pageHref: (trace: DrawerTrace) => string;
  /** What opened the drawer. */
  returnFocusRef: React.RefObject<HTMLElement | null>;
  /** Drawers of the page open under this one, shown over the trace as a way back to each. */
  trail?: React.ReactNode;
}) {
  const { t } = useTranslation('traces');
  const layout = useTraceLayout();
  const windowWidth = useWindowWidth();
  const trace = currentTrace(state);
  const maxWidth = Math.max(TRACE_LAYOUT_LIMITS.drawer.min, windowWidth - LIST_ROOM);
  const width = Math.min(maxWidth, layout.widths.drawer ?? Math.round(windowWidth * TRACE_LAYOUT_LIMITS.drawer.fallbackShare));

  // Read on the way down: a control inside that keeps its keys to itself (a reference does) still lets J and K by.
  const onKeyDownCapture = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    // A floating menu opened from inside is no part of the drawer's page, nor is a field being typed in.
    if (!event.currentTarget.contains(event.target as Node)) return;
    if ((event.target as HTMLElement).closest('input, textarea, [contenteditable="true"], [role="menu"]')) return;
    if (event.key === 'j' && neighbours?.onNext) {
      event.preventDefault();
      neighbours.onNext();
    } else if (event.key === 'k' && neighbours?.onPrevious) {
      event.preventDefault();
      neighbours.onPrevious();
    }
  };

  const actions = trace && (
    <>
      {state && state.stack.length > 1 && (
        <HeadButton label={t('view.drawer.back')} onClick={onBack}>
          <ArrowLeft className="h-4 w-4" />
        </HeadButton>
      )}
      <HeadButton label={t('view.drawer.previous')} onClick={neighbours?.onPrevious} disabled={!neighbours?.onPrevious}>
        <ChevronUp className="h-4 w-4" />
      </HeadButton>
      <HeadButton label={t('view.drawer.next')} onClick={neighbours?.onNext} disabled={!neighbours?.onNext}>
        <ChevronDown className="h-4 w-4" />
      </HeadButton>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button variant="ghost" size="icon" className="h-8 w-8" asChild>
            <Link to={pageHref(trace)} aria-label={t('view.drawer.open_page')} data-testid="trace-drawer-page">
              <Maximize2 className="h-4 w-4" />
            </Link>
          </Button>
        </TooltipTrigger>
        <TooltipContent>{t('view.drawer.open_page')}</TooltipContent>
      </Tooltip>
      <SheetClose asChild>
        <Button variant="ghost" size="icon" className="h-8 w-8" aria-label={t('explore.results.close_detail')}>
          <X className="h-4 w-4" />
        </Button>
      </SheetClose>
    </>
  );

  const view = trace && (
    <TraceView
      key={`${trace.traceId}-${state?.stack.length ?? 0}`}
      traceId={trace.traceId}
      window={trace.window}
      initialSpanId={trace.spanId ?? null}
      frame="drawer"
      actions={actions}
      onOpenTrace={(traceId, spanId, aroundNs) =>
        onFollow({
          traceId,
          spanId,
          ...(aroundNs ? { window: { from: Math.floor(aroundNs / 1_000) - 3_600_000_000, to: Math.floor(aroundNs / 1_000) + 3_600_000_000 } } : {}),
        })
      }
      {...(trail ? { className: 'h-auto min-h-0 flex-1' } : {})}
    />
  );

  return (
    <Sheet open={Boolean(state?.open)} onOpenChange={(open) => !open && onClose()}>
      <SheetContent
        side="right"
        hideClose
        // Under the top bar, and light enough to keep the list in sight.
        overlayClassName="top-topbar bg-overlay-soft"
        aria-describedby={undefined}
        data-trace-drawer=""
        onKeyDownCapture={onKeyDownCapture}
        onCloseAutoFocus={(event) => {
          const opener = returnFocusRef.current;
          if (!opener?.isConnected) return;
          event.preventDefault();
          opener.focus();
        }}
        className={cn('bottom-0 top-topbar flex h-auto max-w-none flex-row gap-0 border-0 bg-[var(--functional-surface)] p-0 shadow-drawer sm:max-w-none')}
        style={{ width }}
      >
        <div className="absolute inset-y-0 left-0 z-30 -translate-x-1/2">
          <ResizeHandle
            label={t('view.drawer.resize')}
            width={width}
            min={TRACE_LAYOUT_LIMITS.drawer.min}
            max={maxWidth}
            grows="left"
            defaultWidth={Math.round(windowWidth * TRACE_LAYOUT_LIMITS.drawer.fallbackShare)}
            onWidthChange={(drawer) => layout.preview({ drawer })}
            onWidthCommit={(drawer) => layout.commit({ drawer })}
            quiet
            className="h-full"
          />
        </div>
        <SheetTitle className="sr-only">{t('detail.title')}</SheetTitle>
        {trail ? (
          <div className="flex min-h-0 min-w-0 flex-1 flex-col">
            <div className="-mb-1 shrink-0 px-3 pt-2 empty:hidden">{trail}</div>
            {view}
          </div>
        ) : (
          view
        )}
      </SheetContent>
    </Sheet>
  );
}

function HeadButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick?: (() => void) | undefined;
  disabled?: boolean | undefined;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="ghost" size="icon" className="h-8 w-8" aria-label={label} onClick={onClick} disabled={disabled}>
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
