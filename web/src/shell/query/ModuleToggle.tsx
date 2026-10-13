import type { LucideIcon } from 'lucide-react';

import { cn } from '@/shell/lib/cn';

import { QueryIconAction } from './IconAction';

interface ModuleToggleProps {
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
 * fields panel). It is a sign like the actions beside it, on the same gray: an
 * open module lights the sign, a closed one dims it.
 */
export function ModuleToggle({
  open,
  icon,
  actionLabel,
  size = 'md',
  onOpenChange,
}: ModuleToggleProps) {
  return (
    <QueryIconAction
      icon={icon}
      label={actionLabel}
      size={size}
      aria-pressed={open}
      data-module-state={open ? 'open' : 'closed'}
      onClick={() => onOpenChange(!open)}
      className={cn(!open && 'text-tx-3 enabled:hover:text-tx-1')}
      iconClassName={cn('transition-colors duration-fast', open && 'text-indigo')}
    />
  );
}
