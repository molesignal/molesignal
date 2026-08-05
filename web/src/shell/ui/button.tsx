import { Slot } from '@radix-ui/react-slot';
import { cva, type VariantProps } from 'class-variance-authority';
import * as React from 'react';

import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';

const buttonVariants = cva(
  'inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 aria-disabled:pointer-events-none aria-disabled:cursor-not-allowed aria-disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0',
  {
    variants: {
      variant: {
        default:
          'bg-primary text-primary-foreground [&:not([aria-disabled=true])]:hover:bg-primary/90',
        destructive:
          'bg-destructive text-destructive-foreground [&:not([aria-disabled=true])]:hover:bg-destructive/90',
        outline:
          'border border-border bg-transparent [&:not([aria-disabled=true])]:hover:bg-muted [&:not([aria-disabled=true])]:hover:text-foreground',
        secondary:
          'bg-secondary text-secondary-foreground [&:not([aria-disabled=true])]:hover:bg-secondary/80',
        ghost:
          '[&:not([aria-disabled=true])]:hover:bg-muted [&:not([aria-disabled=true])]:hover:text-foreground',
        link:
          'text-primary underline-offset-4 [&:not([aria-disabled=true])]:hover:underline',
      },
      size: {
        default: 'h-9 px-3.5 py-1.5',
        sm: 'h-8 rounded-md px-2.5 text-xs',
        lg: 'h-11 rounded-md px-6',
        icon: 'h-9 w-9',
      },
    },
    defaultVariants: { variant: 'default', size: 'default' },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
  disabledReason?: React.ReactNode;
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  (
    {
      className,
      variant,
      size,
      asChild = false,
      disabled = false,
      disabledReason,
      onClick,
      ...props
    },
    ref,
  ) => {
    const Comp = asChild ? Slot : 'button';
    const button = (
      <Comp
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        {...props}
        {...(!asChild && { disabled })}
        aria-disabled={disabled || undefined}
        tabIndex={asChild && disabled ? -1 : props.tabIndex}
        onClick={(event) => {
          if (disabled) {
            event.preventDefault();
            event.stopPropagation();
            return;
          }
          onClick?.(event);
        }}
      />
    );
    return (
      <DisabledControl disabled={disabled} reason={disabledReason}>
        {button}
      </DisabledControl>
    );
  },
);
Button.displayName = 'Button';

export { buttonVariants };
