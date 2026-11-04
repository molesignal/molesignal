import type * as React from 'react';

import { cn } from '@/shell/lib/cn';

/** Both faces sit in one grid cell, so the larger sets the size and neither moves the other. */
const FACE =
  'col-start-1 row-start-1 grid place-items-center transition-[opacity,filter,transform] duration-300 ease-[cubic-bezier(0.2,0,0,1)]';
const SHOWN = 'scale-100 opacity-100 blur-0';
const HIDDEN = 'scale-[0.25] opacity-0 blur-[4px]';

/**
 * The icon of a control that changes with its state (copy → copied, pin → pinned, play → pause).
 * Both icons stay on the page and cross-fade: the one leaving shrinks to a quarter, blurs and
 * fades while the other grows in, with no overshoot. Being transitions, a change that turns back
 * halfway turns back from where it is. The state itself has to be said elsewhere too (a colour,
 * a label): the motion is never the only sign of it.
 */
export function IconSwap({
  active,
  inactiveIcon,
  activeIcon,
  className,
}: {
  active: boolean;
  /** Shown while `active` is false. */
  inactiveIcon: React.ReactNode;
  /** Shown while `active` is true. */
  activeIcon: React.ReactNode;
  className?: string;
}) {
  return (
    <span className={cn('inline-grid place-items-center', className)}>
      <span aria-hidden={active || undefined} className={cn(FACE, active ? HIDDEN : SHOWN)}>
        {inactiveIcon}
      </span>
      <span aria-hidden={!active || undefined} className={cn(FACE, active ? SHOWN : HIDDEN)}>
        {activeIcon}
      </span>
    </span>
  );
}
