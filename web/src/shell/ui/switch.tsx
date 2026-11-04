import * as SwitchPrimitive from '@radix-ui/react-switch';
import * as React from 'react';

import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';

type SwitchProps = React.ComponentPropsWithoutRef<typeof SwitchPrimitive.Root> & {
  disabledReason?: React.ReactNode;
};

const Switch = React.forwardRef<
  React.ElementRef<typeof SwitchPrimitive.Root>,
  SwitchProps
>(({ className, disabled, disabledReason, ...props }, ref) => {
  const control = (
    <SwitchPrimitive.Root
      ref={ref}
      disabled={disabled}
      aria-disabled={disabled || undefined}
      className={cn(
        'peer relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border-2 border-transparent transition-colors after:absolute after:-inset-x-1 after:-inset-y-2 after:content-[""] disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-100 data-[state=checked]:bg-primary data-[state=unchecked]:bg-input disabled:data-[state=checked]:bg-[var(--tx-disabled)] disabled:data-[state=unchecked]:bg-bg-3',
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        className={cn(
          'pointer-events-none block h-3 w-3 rounded-full bg-bg shadow-lg ring-0 transition-transform duration-200 ease-spring data-[state=checked]:translate-x-4 data-[state=unchecked]:translate-x-0',
        )}
      />
    </SwitchPrimitive.Root>
  );
  return (
    <DisabledControl disabled={Boolean(disabled)} reason={disabledReason}>
      {control}
    </DisabledControl>
  );
});
Switch.displayName = SwitchPrimitive.Root.displayName;

export { Switch };
