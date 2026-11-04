import * as CheckboxPrimitive from '@radix-ui/react-checkbox';
import { Check, Minus } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

const Checkbox = React.forwardRef<
  React.ElementRef<typeof CheckboxPrimitive.Root>,
  React.ComponentPropsWithoutRef<typeof CheckboxPrimitive.Root>
>(({ className, ...props }, ref) => (
  <CheckboxPrimitive.Root
    ref={ref}
    className={cn(
      'ms-press peer grid h-4 w-4 shrink-0 place-content-center rounded-[4px] border border-bd-2 bg-bg-1 text-white shadow-none',
      'disabled:cursor-not-allowed disabled:border-transparent disabled:bg-bg-3',
      'data-[state=checked]:border-indigo data-[state=checked]:bg-indigo data-[state=checked]:text-white',
      'data-[state=indeterminate]:border-indigo data-[state=indeterminate]:bg-indigo data-[state=indeterminate]:text-white',
      'disabled:data-[state=checked]:border-transparent disabled:data-[state=checked]:bg-[var(--tx-disabled)]',
      'disabled:data-[state=indeterminate]:border-transparent disabled:data-[state=indeterminate]:bg-[var(--tx-disabled)]',
      className,
    )}
    {...props}
  >
    <CheckboxPrimitive.Indicator
      className={cn('grid place-content-center text-current')}
    >
      {props.checked === 'indeterminate' ? (
        <Minus className="h-3 w-3 stroke-[3px]" />
      ) : (
        <Check className="h-3 w-3 stroke-[3px]" />
      )}
    </CheckboxPrimitive.Indicator>
  </CheckboxPrimitive.Root>
));
Checkbox.displayName = CheckboxPrimitive.Root.displayName;

export { Checkbox };
