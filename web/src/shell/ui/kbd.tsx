import { cva, type VariantProps } from 'class-variance-authority';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

/**
 * Kbd — render a keyboard key chip. Custom shadcn-style component (shadcn has
 * no kbd primitive). Used in palette footer, help overlay, and shell chrome.
 *
 * Usage:
 *   <Kbd>⌘K</Kbd>
 *   <Kbd size="sm">Esc</Kbd>
 *   <Kbd>g</Kbd><Kbd>s</Kbd>   // sequence
 */
const kbdVariants = cva(
  'inline-flex items-center justify-center font-sans font-medium border border-border bg-surface-muted text-muted-foreground rounded-sm shadow-[inset_0_-1px_0_var(--border)]',
  {
    variants: {
      size: {
        default: 'h-6 min-w-6 px-1.5 text-xs',
        sm: 'type-micro h-5 min-w-5 px-1',
        lg: 'h-6 min-w-[1.5rem] px-2 text-xs',
      },
    },
    defaultVariants: { size: 'default' },
  },
);

export interface KbdProps
  extends React.HTMLAttributes<HTMLElement>,
    VariantProps<typeof kbdVariants> {}

export const Kbd = React.forwardRef<HTMLElement, KbdProps>(
  ({ className, size, children, ...props }, ref) => (
    <kbd ref={ref} className={cn(kbdVariants({ size }), className)} {...props}>
      {children}
    </kbd>
  ),
);
Kbd.displayName = 'Kbd';
