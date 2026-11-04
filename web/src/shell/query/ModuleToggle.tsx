import type { LucideIcon } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { QueryIconAction } from './IconAction';

interface ModuleToggleProps
  extends Omit<
    React.ComponentPropsWithoutRef<typeof QueryIconAction>,
    'icon' | 'label' | 'size' | 'onClick' | 'aria-pressed'
  > {
  /** Whether the module is open right now. */
  open: boolean;
  icon: LucideIcon;
  /** What pressing the button does: its name, and the tip under the pointer. */
  actionLabel: string;
  /** `sm` for a results header that has little room. */
  size?: 'md' | 'sm';
  onOpenChange: (open: boolean) => void;
}

/**
 * Opens or closes one module around an explorer's results (the histogram, the
 * fields panel, the other half of the page). It is a sign like the actions
 * beside it, on the same gray: an open module lights the sign, a closed one
 * dims it.
 */
export const ModuleToggle = React.forwardRef<HTMLButtonElement, ModuleToggleProps>(function ModuleToggle(
  { open, icon, actionLabel, size = 'md', onOpenChange, className, iconClassName, ...button },
  ref,
) {
  return (
    <QueryIconAction
      ref={ref}
      {...button}
      icon={icon}
      label={actionLabel}
      size={size}
      aria-pressed={open}
      data-module-state={open ? 'open' : 'closed'}
      onClick={() => onOpenChange(!open)}
      className={cn(!open && 'text-tx-3 enabled:hover:text-tx-1', className)}
      iconClassName={cn('transition-colors duration-fast', open && 'text-indigo', iconClassName)}
    />
  );
});
