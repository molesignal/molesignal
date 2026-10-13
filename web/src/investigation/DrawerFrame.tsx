import { Pin, PinOff, X } from 'lucide-react';
import * as React from 'react';

import { useScope } from '@/keyboard/controller';
import { cn } from '@/shell/lib/cn';
import { Badge } from '@/shell/ui/badge';
import { Kbd } from '@/shell/ui/kbd';
import { Separator } from '@/shell/ui/separator';
import { type Frame, useInvestigationStack } from '@/stores/useInvestigationStack';

import { FRAME_KIND_LABELS, FrameRenderer } from './frame';

interface DrawerFrameProps {
  frame: Frame;
  /** Index from bottom; 0 is the lowest drawer. */
  index: number;
  /** Total number of drawers currently in the stack. */
  total: number;
  isTop: boolean;
  /**
   * Set once the frame has left the stack and its drawer is sliding out: how
   * long the drawer waits before it moves. `index` and `total` are then the
   * ones the frame had when it left, so the drawer leaves from where it stood.
   */
  leaveDelayMs?: number;
}

const DRAWER_W = 720;
const STACK_OFFSET = 32;

/** A click on a drawer that is leaving reaches nothing: not its controls, not what it still covers. */
function dropClick(event: React.MouseEvent) {
  event.preventDefault();
  event.stopPropagation();
}

export function DrawerFrame({ frame, index, total, isTop, leaveDelayMs }: DrawerFrameProps) {
  useScope('drawer', isTop);
  const pop = useInvestigationStack((s) => s.pop);
  const pin = useInvestigationStack((s) => s.pinFrame);
  const popTo = useInvestigationStack((s) => s.popTo);
  const leaving = leaveDelayMs !== undefined;
  // The slide-in belongs to arriving, not to being on top. A drawer that
  // becomes the top one because the one above it closed was here all along,
  // and sliding in again would cross the drawer that is sliding out over it.
  const [arrivedOnTop] = React.useState(isTop);

  // Higher index → closer to user. Lower index → peek 32px from the right edge
  // of the next drawer.
  const offsetFromRight = (total - 1 - index) * STACK_OFFSET;

  return (
    <aside
      role="dialog"
      aria-modal={isTop}
      aria-labelledby={`drawer-${frame.id}-title`}
      data-frame-id={frame.id}
      data-frame-kind={frame.kind}
      data-frame-index={index}
      data-leaving={leaving || undefined}
      style={{
        width: DRAWER_W,
        right: offsetFromRight,
        zIndex: 60 + index,
        // Out to the right by its own width plus the distance it stood back
        // from the edge, so a drawer from lower in the stack leaves the screen too.
        ...(leaving && {
          translate: `calc(100% + ${offsetFromRight}px)`,
          transitionDelay: `${leaveDelayMs}ms`,
        }),
      }}
      className={cn(
        'fixed top-strip bottom-0 flex flex-col border-l border-border bg-surface shadow-drawer',
        arrivedOnTop && 'animate-slide-in-right',
        leaving && 'transition-[translate] duration-normal ease-out',
      )}
      onClickCapture={leaving ? dropClick : undefined}
      onClick={(e) => {
        if (!isTop) {
          e.stopPropagation();
          popTo(frame.id);
        }
      }}
    >
      <header className="flex items-center justify-between gap-2 border-b border-border px-3 py-1.5">
        <div className="flex items-center gap-2 text-xs">
          <Badge variant="outline" id={`drawer-${frame.id}-title`}>
            {FRAME_KIND_LABELS[frame.kind]}
          </Badge>
          {frame.parent_frame_id && (
            <span className="text-muted-foreground">from #{frame.parent_frame_id.slice(0, 4)}</span>
          )}
        </div>
        <div className="flex items-center gap-1.5">
          {isTop && (
            <span className="flex items-center gap-1 text-xs text-muted-foreground">
              <Kbd size="sm">Esc</Kbd> close
            </span>
          )}
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              pin(frame.id, !frame.pinned);
            }}
            className="rounded p-1 text-muted-foreground hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-label={frame.pinned ? 'Unpin frame' : 'Pin frame'}
          >
            {frame.pinned ? <Pin className="h-3.5 w-3.5" /> : <PinOff className="h-3.5 w-3.5" />}
          </button>
          <Separator orientation="vertical" className="h-4" />
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              pop();
            }}
            className="rounded p-1 text-muted-foreground hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-label="Close frame"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      </header>
      <div className="min-h-0 flex-1 overflow-auto">
        <FrameRenderer frame={frame} />
      </div>
    </aside>
  );
}
