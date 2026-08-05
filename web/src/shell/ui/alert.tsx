import { cva, type VariantProps } from 'class-variance-authority';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

/**
 * Alert / Banner — Phase 4 token-aware shadcn primitive.
 *
 * Brief mandate: 4 semantic variants matching the status color tokens
 * (info/success/warn/error). Icons are NOT embedded — caller passes a
 * lucide stroke icon as the first child so the variant set stays
 * orthogonal to icon choice.
 *
 * Page-level use: this is for sticky notices that explain a state ("3
 * queries failed", "license expires in 7 days"). Use Toast for transient
 * confirmations, ErrorState for the in-place obstacle.
 */

const alertVariants = cva(
  cn(
    'relative w-full rounded-md border px-4 py-3 font-sans text-xs',
    '[&>svg+div]:translate-y-[-1px]',
    '[&>svg]:absolute [&>svg]:left-4 [&>svg]:top-3.5 [&>svg]:h-4 [&>svg]:w-4 [&>svg]:stroke-[1.6]',
    '[&>svg~*]:pl-6',
  ),
  {
    variants: {
      variant: {
        default: 'border-bd-1 bg-bg-1 text-tx-1 [&>svg]:text-tx-2',
        info: 'border-blue/30 bg-blue-dim text-blue-soft [&>svg]:text-blue',
        success: 'border-green/30 bg-green-dim text-green-soft [&>svg]:text-green',
        warning: 'border-yellow/30 bg-yellow-dim text-yellow-soft [&>svg]:text-yellow',
        // Status-tagged danger surface. Mirrors brief: `red` is the
        // error/firing token, bumped to AAA on bg-0.
        destructive: 'border-red/30 bg-red-dim text-red-soft [&>svg]:text-red',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  },
);

const Alert = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement> & VariantProps<typeof alertVariants>
>(({ className, variant, ...props }, ref) => (
  <div ref={ref} role="alert" className={cn(alertVariants({ variant }), className)} {...props} />
));
Alert.displayName = 'Alert';

const AlertTitle = React.forwardRef<HTMLParagraphElement, React.HTMLAttributes<HTMLHeadingElement>>(
  ({ className, ...props }, ref) => (
    <h5
      ref={ref}
      className={cn(
        'mb-0.5 font-sans text-xs font-display-strong leading-tight tracking-tight text-tx-0',
        className,
      )}
      {...props}
    />
  ),
);
AlertTitle.displayName = 'AlertTitle';

const AlertDescription = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
  <div ref={ref} className={cn('text-xs leading-relaxed [&_p]:leading-relaxed', className)} {...props} />
));
AlertDescription.displayName = 'AlertDescription';

export { Alert, AlertDescription, AlertTitle };
