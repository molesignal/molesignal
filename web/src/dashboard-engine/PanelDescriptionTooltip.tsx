import { Info } from 'lucide-react';
import * as React from 'react';

import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

export interface PanelDescriptionTooltipProps {
  description: string;
  label: string;
  panelTitle: string;
}

export function PanelDescriptionTooltip({
  description,
  label,
  panelTitle,
}: PanelDescriptionTooltipProps) {
  const [open, setOpen] = React.useState(false);
  const pinnedRef = React.useRef(false);
  const triggerRef = React.useRef<HTMLButtonElement>(null);

  const dismiss = React.useCallback(() => {
    pinnedRef.current = false;
    setOpen(false);
  }, []);

  return (
    <Tooltip
      open={open}
      onOpenChange={(nextOpen) => {
        if (nextOpen || !pinnedRef.current) setOpen(nextOpen);
      }}
    >
      <TooltipTrigger asChild>
        <button
          ref={triggerRef}
          type="button"
          aria-expanded={open}
          aria-label={`${label}: ${panelTitle}`}
          className="grid h-6 w-6 shrink-0 place-items-center rounded text-tx-3 transition-colors hover:bg-bg-3 hover:text-tx-1 focus-visible:bg-bg-3 focus-visible:text-accent"
          onBlur={() => {
            pinnedRef.current = false;
          }}
          onClick={(event) => {
            event.preventDefault();
            event.stopPropagation();
            const nextPinned = !pinnedRef.current;
            pinnedRef.current = nextPinned;
            setOpen(nextPinned);
          }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <Info aria-hidden="true" className="h-3.5 w-3.5" />
        </button>
      </TooltipTrigger>
      <TooltipContent
        side="top"
        align="start"
        className="max-w-sm px-2.5 py-2 leading-relaxed"
        onEscapeKeyDown={dismiss}
        onPointerDownOutside={(event) => {
          const target = event.detail.originalEvent.target;
          if (target instanceof Node && triggerRef.current?.contains(target)) {
            event.preventDefault();
            return;
          }
          dismiss();
        }}
      >
        {description}
      </TooltipContent>
    </Tooltip>
  );
}
