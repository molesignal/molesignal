import { cva, type VariantProps } from 'class-variance-authority';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

/**
 * Kbd — render a keyboard key. Custom shadcn-style component (shadcn has
 * no kbd primitive). Used in palette footer, help overlay, and shell chrome.
 *
 * Usage:
 *   <Kbd>⌘K</Kbd>
 *   <Kbd size="sm">Esc</Kbd>
 *   <Kbd>g</Kbd><Kbd>s</Kbd>   // sequence
 *   <Kbd variant="plain">⌘ K</Kbd>   // the sign and the letter alone
 */
const kbdVariants = cva('inline-flex items-center justify-center font-sans', {
  variants: {
    variant: {
      // A key cap: a shortcut listed in a menu, the palette footer, the help overlay.
      cap: 'rounded-sm border border-border bg-surface-muted font-medium text-muted-foreground shadow-[inset_0_-1px_0_var(--border)]',
      // No cap, light words: a shortcut shown inside a control that already says what it does
      // (the top bar's search), where a cap would draw a second control.
      plain: 'font-normal text-[var(--shortcut-hint)]',
    },
    size: {
      default: 'h-6 min-w-6 px-1.5 text-xs',
      sm: 'type-micro h-5 min-w-5 px-1',
      lg: 'h-6 min-w-[1.5rem] px-2 text-xs',
    },
  },
  compoundVariants: [{ variant: 'plain', className: 'h-auto min-w-0 px-0' }],
  defaultVariants: { variant: 'cap', size: 'default' },
});

export interface KbdProps
  extends React.HTMLAttributes<HTMLElement>,
    VariantProps<typeof kbdVariants> {}

export const Kbd = React.forwardRef<HTMLElement, KbdProps>(
  ({ className, variant, size, children, ...props }, ref) => (
    <kbd ref={ref} className={cn(kbdVariants({ variant, size }), className)} {...props}>
      {children}
    </kbd>
  ),
);
Kbd.displayName = 'Kbd';
