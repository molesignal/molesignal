import * as DropdownMenuPrimitive from '@radix-ui/react-dropdown-menu';
import { Check, ChevronRight } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import {
  floatingMenuContentClass,
  floatingMenuItemClass,
  floatingMenuLabelClass,
  floatingMenuSelectedClass,
  floatingMenuSeparatorClass,
} from '@/shell/ui/floating';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

const DropdownMenu = DropdownMenuPrimitive.Root;
const DropdownMenuTrigger = DropdownMenuPrimitive.Trigger;
const DropdownMenuGroup = DropdownMenuPrimitive.Group;
const DropdownMenuPortal = DropdownMenuPrimitive.Portal;
const DropdownMenuSub = DropdownMenuPrimitive.Sub;
const DropdownMenuRadioGroup = DropdownMenuPrimitive.RadioGroup;

/** The distance a menu keeps from the edges of the window, the same a tooltip keeps. */
const WINDOW_EDGE_GAP = 8;

/**
 * A menu taller than the room it opens in scrolls inside that room, so its last items stay in
 * reach on a short screen. The room is what Radix measured on the side the menu opened, short of
 * the window's edge by `WINDOW_EDGE_GAP`. The scroll padding is the menu's own, so an item the
 * keyboard brings into view keeps its distance from the edge.
 *
 * A `max-h` given by a caller replaces this one. A caller with a limit of its own keeps the room
 * in it: `max-h-[min(420px,var(--radix-dropdown-menu-content-available-height))]`.
 */
const dropdownMenuScrollClass =
  'max-h-[var(--radix-dropdown-menu-content-available-height)] overflow-y-auto scroll-py-[4px]';

const DropdownMenuSubTrigger = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.SubTrigger>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.SubTrigger> & {
    disabledReason?: React.ReactNode;
    inset?: boolean;
  }
>(({ className, disabled, disabledReason, inset, children, ...props }, ref) => {
  const trigger = (
    <DropdownMenuPrimitive.SubTrigger
      ref={ref}
      {...(disabled !== undefined ? { disabled } : {})}
      aria-disabled={disabled || undefined}
      className={cn(
        floatingMenuItemClass,
        'data-[state=open]:bg-[var(--floating-item-hover)] data-[state=open]:text-tx-0',
        inset && 'pl-8',
        className,
      )}
      {...props}
    >
      {children}
      <ChevronRight className="ml-auto h-[14px] w-[14px]" />
    </DropdownMenuPrimitive.SubTrigger>
  );
  if (!disabled || !disabledReason) return trigger;
  return (
    <Tooltip>
      <TooltipTrigger asChild>{trigger}</TooltipTrigger>
      <TooltipContent side="left" className="max-w-xs leading-relaxed">
        {disabledReason}
      </TooltipContent>
    </Tooltip>
  );
});
DropdownMenuSubTrigger.displayName = DropdownMenuPrimitive.SubTrigger.displayName;

const DropdownMenuSubContent = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.SubContent>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.SubContent>
>(({ className, collisionPadding = WINDOW_EDGE_GAP, ...props }, ref) => (
  <DropdownMenuPrimitive.SubContent
    ref={ref}
    collisionPadding={collisionPadding}
    className={cn(
      floatingMenuContentClass,
      dropdownMenuScrollClass,
      'origin-[var(--radix-dropdown-menu-content-transform-origin)] data-[state=open]:animate-pop-in data-[state=closed]:animate-pop-out',
      className,
    )}
    {...props}
  />
));
DropdownMenuSubContent.displayName = DropdownMenuPrimitive.SubContent.displayName;

const DropdownMenuContent = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Content>
>(({ className, sideOffset = 4, collisionPadding = WINDOW_EDGE_GAP, ...props }, ref) => (
  <DropdownMenuPrimitive.Portal>
    <DropdownMenuPrimitive.Content
      ref={ref}
      sideOffset={sideOffset}
      collisionPadding={collisionPadding}
      className={cn(
        floatingMenuContentClass,
        dropdownMenuScrollClass,
        'origin-[var(--radix-dropdown-menu-content-transform-origin)] data-[state=open]:animate-pop-in data-[state=closed]:animate-pop-out',
        className,
      )}
      {...props}
    />
  </DropdownMenuPrimitive.Portal>
));
DropdownMenuContent.displayName = DropdownMenuPrimitive.Content.displayName;

const DropdownMenuItem = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.Item>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Item> & {
    disabledReason?: React.ReactNode;
    inset?: boolean;
  }
>(({ className, disabled, disabledReason, inset, ...props }, ref) => {
  const item = (
    <DropdownMenuPrimitive.Item
      ref={ref}
      aria-disabled={disabled || undefined}
      {...(disabled !== undefined ? { disabled } : {})}
      className={cn(
        floatingMenuItemClass,
        inset && 'pl-8',
        className,
      )}
      {...props}
    />
  );

  if (!disabled || !disabledReason) return item;

  return (
    <Tooltip>
      <TooltipTrigger asChild>{item}</TooltipTrigger>
      <TooltipContent side="left" className="max-w-xs leading-relaxed">
        {disabledReason}
      </TooltipContent>
    </Tooltip>
  );
});
DropdownMenuItem.displayName = DropdownMenuPrimitive.Item.displayName;

const DropdownMenuCheckboxItem = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.CheckboxItem>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.CheckboxItem>
>(({ className, children, checked, ...props }, ref) => (
  <DropdownMenuPrimitive.CheckboxItem
    ref={ref}
    className={cn(
      floatingMenuItemClass,
      floatingMenuSelectedClass,
      'pl-[30px] pr-[10px]',
      className,
    )}
    {...(checked !== undefined && { checked })}
    {...props}
  >
    <span className="absolute left-[8px] flex h-[14px] w-[14px] items-center justify-center">
      <DropdownMenuPrimitive.ItemIndicator>
        <Check className="h-[14px] w-[14px] text-indigo-soft" />
      </DropdownMenuPrimitive.ItemIndicator>
    </span>
    {children}
  </DropdownMenuPrimitive.CheckboxItem>
));
DropdownMenuCheckboxItem.displayName = DropdownMenuPrimitive.CheckboxItem.displayName;

const DropdownMenuRadioItem = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.RadioItem>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.RadioItem>
>(({ className, children, ...props }, ref) => (
  <DropdownMenuPrimitive.RadioItem
    ref={ref}
    className={cn(
      floatingMenuItemClass,
      floatingMenuSelectedClass,
      'pl-[30px] pr-[10px]',
      className,
    )}
    {...props}
  >
    <span className="absolute left-[8px] flex h-[14px] w-[14px] items-center justify-center">
      <DropdownMenuPrimitive.ItemIndicator>
        <Check className="h-[14px] w-[14px] text-indigo-soft" />
      </DropdownMenuPrimitive.ItemIndicator>
    </span>
    {children}
  </DropdownMenuPrimitive.RadioItem>
));
DropdownMenuRadioItem.displayName = DropdownMenuPrimitive.RadioItem.displayName;

const DropdownMenuLabel = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.Label>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Label> & { inset?: boolean }
>(({ className, inset, ...props }, ref) => (
  <DropdownMenuPrimitive.Label
    ref={ref}
    className={cn(
      floatingMenuLabelClass,
      inset && 'pl-8',
      className,
    )}
    {...props}
  />
));
DropdownMenuLabel.displayName = DropdownMenuPrimitive.Label.displayName;

const DropdownMenuSeparator = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.Separator>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Separator>
>(({ className, ...props }, ref) => (
  <DropdownMenuPrimitive.Separator
    ref={ref}
    className={cn(floatingMenuSeparatorClass, className)}
    {...props}
  />
));
DropdownMenuSeparator.displayName = DropdownMenuPrimitive.Separator.displayName;

const DropdownMenuShortcut = ({ className, ...props }: React.HTMLAttributes<HTMLSpanElement>) => (
  <span
    className={cn('ml-auto type-micro tracking-wide text-tx-3', className)}
    {...props}
  />
);
DropdownMenuShortcut.displayName = 'DropdownMenuShortcut';

export {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuCheckboxItem,
  DropdownMenuRadioItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuGroup,
  DropdownMenuPortal,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuRadioGroup,
};
