import { cva, type VariantProps } from 'class-variance-authority';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

const badgeVariants = cva(
  'inline-flex items-center gap-1 rounded-md border px-2 py-0.5 text-xs font-medium transition-colors',
  {
    variants: {
      variant: {
        default: 'border-transparent bg-primary text-primary-foreground hover:bg-primary/80',
        secondary: 'border-transparent bg-secondary text-secondary-foreground hover:bg-secondary/80',
        destructive: 'border-transparent bg-destructive text-destructive-foreground hover:bg-destructive/80',
        outline: 'border-border text-foreground',
        accent: 'border-transparent bg-accent-bg text-accent',
      },
    },
    defaultVariants: { variant: 'default' },
  },
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLDivElement>,
    VariantProps<typeof badgeVariants> {}

export const Badge = React.forwardRef<HTMLDivElement, BadgeProps>(
  function Badge({ className, variant, ...props }, ref) {
    return (
      <div
        ref={ref}
        className={cn(badgeVariants({ variant }), className)}
        {...props}
      />
    );
  },
);

export { badgeVariants };
