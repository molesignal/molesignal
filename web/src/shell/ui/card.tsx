import * as React from 'react';

import { cn } from '@/shell/lib/cn';

/**
 * Card — Phase 4 token-aware shadcn primitive.
 *
 * Brief mandate: cards distinguish via 1px border (bd-0) over surface
 * (bg-1). No shadow. Radius 6px (md), not 12px. Padding 16px (p-4)
 * instead of the default p-6 — density-first.
 *
 * The shadcn shadow has been deliberately removed: Confident-quiet does
 * not bloom. Use border-bd-1 to bump emphasis instead of shadow.
 */

const Card = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div
      ref={ref}
      className={cn(
        'rounded-md border border-bd-0 bg-surface text-tx-0',
        className,
      )}
      {...props}
    />
  ),
);
Card.displayName = 'Card';

const CardHeader = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div ref={ref} className={cn('flex flex-col gap-1 p-4', className)} {...props} />
  ),
);
CardHeader.displayName = 'CardHeader';

const CardTitle = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div
      ref={ref}
      className={cn('font-sans text-sm font-display-strong leading-tight text-tx-0', className)}
      {...props}
    />
  ),
);
CardTitle.displayName = 'CardTitle';

const CardDescription = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div ref={ref} className={cn('font-sans text-xs text-tx-2', className)} {...props} />
  ),
);
CardDescription.displayName = 'CardDescription';

const CardContent = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div ref={ref} className={cn('px-4 pb-4 pt-0', className)} {...props} />
  ),
);
CardContent.displayName = 'CardContent';

const CardFooter = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div
      ref={ref}
      className={cn(
        'flex items-center gap-2 border-t border-bd-0 px-4 py-3',
        className,
      )}
      {...props}
    />
  ),
);
CardFooter.displayName = 'CardFooter';

export { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle };
