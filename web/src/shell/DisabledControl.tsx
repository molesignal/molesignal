import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

/**
 * Native disabled controls do not emit pointer events, so Radix cannot use
 * them as tooltip triggers. The wrapper owns the disabled cursor and tooltip
 * while the control keeps native keyboard/click suppression.
 */
export function DisabledControl({
  disabled,
  reason,
  children,
  className,
}: {
  disabled: boolean;
  reason?: React.ReactNode;
  children: React.ReactElement;
  className?: string;
}) {
  if (!disabled) return children;

  const trigger = (
    <span
      className={cn('inline-flex shrink-0 cursor-not-allowed', className)}
      data-disabled-control=""
      aria-disabled="true"
      onClickCapture={(event) => {
        event.preventDefault();
        event.stopPropagation();
      }}
      onKeyDownCapture={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          event.stopPropagation();
        }
      }}
    >
      {children}
    </span>
  );

  if (!reason) return trigger;

  return (
    <TooltipProvider delayDuration={300}>
      <Tooltip>
        <TooltipTrigger asChild>{trigger}</TooltipTrigger>
        <TooltipContent side="top" className="max-w-xs leading-relaxed">
          {reason}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
