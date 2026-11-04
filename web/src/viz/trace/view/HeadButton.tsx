import type { LucideIcon } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

/** A focus this soon after a menu the button opened has closed is the menu handing it back. */
const HANDED_BACK_MS = 250;

interface TraceHeadButtonProps extends Omit<React.ComponentPropsWithoutRef<typeof Button>, 'aria-label' | 'children'> {
  icon: LucideIcon;
  /** What the button does: its name, and the tip it shows. */
  label: string;
}

/**
 * A button of the trace's head, said by its sign: what it does is in its tip
 * and its name. It can open a menu or a popover; the focus handed back as
 * that closes does not bring the tip up.
 */
export const TraceHeadButton = React.forwardRef<HTMLButtonElement, TraceHeadButtonProps>(function TraceHeadButton(
  { icon: Icon, label, className, onFocus, ...button },
  ref,
) {
  const expanded = button['aria-expanded'] === true || button['aria-expanded'] === 'true';
  const wasExpandedRef = React.useRef(expanded);
  const closedAtRef = React.useRef(Number.NEGATIVE_INFINITY);

  React.useLayoutEffect(() => {
    if (wasExpandedRef.current && !expanded) closedAtRef.current = performance.now();
    wasExpandedRef.current = expanded;
  }, [expanded]);

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          ref={ref}
          variant="ghost"
          size="icon"
          {...button}
          aria-label={label}
          onFocus={(event) => {
            onFocus?.(event);
            if (performance.now() - closedAtRef.current < HANDED_BACK_MS) event.preventDefault();
          }}
          className={cn('h-8 w-8', className)}
        >
          <Icon aria-hidden="true" className="h-4 w-4" />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
});
