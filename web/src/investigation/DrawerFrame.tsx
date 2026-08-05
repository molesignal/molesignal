import { Pin, PinOff, X } from 'lucide-react';

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
}

const DRAWER_W = 720;
const STACK_OFFSET = 32;

export function DrawerFrame({ frame, index, total, isTop }: DrawerFrameProps) {
  useScope('drawer', isTop);
  const pop = useInvestigationStack((s) => s.pop);
  const pin = useInvestigationStack((s) => s.pinFrame);
  const popTo = useInvestigationStack((s) => s.popTo);

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
      style={{ width: DRAWER_W, right: offsetFromRight, zIndex: 60 + index }}
      className={cn(
        'fixed top-strip bottom-0 flex flex-col border-l border-border bg-surface shadow-drawer',
        isTop && 'animate-slide-in-right',
      )}
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
