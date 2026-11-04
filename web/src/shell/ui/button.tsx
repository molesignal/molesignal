import { Slot } from '@radix-ui/react-slot';
import { cva, type VariantProps } from 'class-variance-authority';
import * as React from 'react';

import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';
import { hasVisibleText } from '@/shell/lib/hasVisibleText';

const buttonVariants = cva(
  // A disabled button keeps its shape and says so in the pale disabled grey; a filled one
  // turns the quiet fill.
  'ms-press inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium disabled:pointer-events-none disabled:cursor-not-allowed disabled:text-[var(--tx-disabled)] aria-disabled:pointer-events-none aria-disabled:cursor-not-allowed aria-disabled:text-[var(--tx-disabled)] [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0',
  {
    variants: {
      variant: {
        default:
          'bg-primary text-primary-foreground aria-disabled:bg-bg-2 [&:not([aria-disabled=true])]:hover:bg-primary/90',
        destructive:
          'bg-destructive text-destructive-foreground aria-disabled:bg-bg-2 [&:not([aria-disabled=true])]:hover:bg-destructive/90',
        // A quiet button is a filled one, not a bordered one: the product's
        // buttons carry no outline, only a gray fill that deepens on hover.
        outline:
          'border-0 bg-bg-2 text-tx-1 [&:not([aria-disabled=true])]:hover:bg-bg-3 [&:not([aria-disabled=true])]:hover:text-tx-0',
        secondary:
          'bg-secondary text-secondary-foreground aria-disabled:bg-bg-2 [&:not([aria-disabled=true])]:hover:bg-secondary/80',
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
    // Words get room at either side and their icon is drawn at their size (polish/icons.css), as
    // in `ChromeButton`; the large and the icon sizes keep their own.
    const words =
      (size == null || size === 'default' || size === 'sm') && hasVisibleText(props.children)
        ? size === 'sm' ? 'sm' : 'md'
        : undefined;
    const button = (
      <Comp
        className={cn(
          buttonVariants({
            variant,
            size,
            className: cn(words === 'md' && 'px-4', words === 'sm' && 'px-3', className),
          }),
        )}
        ref={ref}
        data-text-button={words}
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
