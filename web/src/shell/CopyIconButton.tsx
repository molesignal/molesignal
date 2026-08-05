import { Check, Copy } from 'lucide-react';
import * as React from 'react';

import { IconButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

export interface CopyIconButtonProps
  extends Omit<
    React.ComponentPropsWithoutRef<typeof IconButton>,
    'aria-label' | 'children' | 'title'
  > {
  label: string;
  copied?: boolean;
  copiedLabel?: string;
  iconClassName?: string;
  wrapperClassName?: string;
  tooltipSide?: React.ComponentPropsWithoutRef<typeof TooltipContent>['side'];
}

/**
 * Project-wide clipboard action: one visible icon, with its full meaning
 * exposed through the accessible name and tooltip.
 */
export const CopyIconButton = React.forwardRef<
  HTMLButtonElement,
  CopyIconButtonProps
>(function CopyIconButton(
  {
    label,
    copied = false,
    copiedLabel,
    iconClassName,
    wrapperClassName,
    tooltipSide = 'top',
    disabled,
    disabledReason,
    className,
    ...buttonProps
  },
  ref,
) {
  const tooltipId = React.useId();
  const statusLabel = copied ? (copiedLabel ?? label) : label;
  const tooltip = disabled && disabledReason ? disabledReason : statusLabel;

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className={cn('inline-flex shrink-0', wrapperClassName)}>
          <IconButton
            ref={ref}
            {...buttonProps}
            disabled={disabled}
            aria-label={statusLabel}
            aria-describedby={
              disabledReason ? tooltipId : buttonProps['aria-describedby']
            }
            className={className}
          >
            {copied ? (
              <Check
                aria-hidden="true"
                className={cn('h-3.5 w-3.5 text-green-soft', iconClassName)}
              />
            ) : (
              <Copy
                aria-hidden="true"
                className={cn('h-3.5 w-3.5', iconClassName)}
              />
            )}
          </IconButton>
        </span>
      </TooltipTrigger>
      <TooltipContent id={tooltipId} side={tooltipSide}>
        {tooltip}
      </TooltipContent>
      {copied && copiedLabel ? (
        <span className="sr-only" role="status" aria-live="polite">
          {copiedLabel}
        </span>
      ) : null}
    </Tooltip>
  );
});
