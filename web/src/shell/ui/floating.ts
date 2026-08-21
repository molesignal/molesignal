/** Shared visual language for lightweight workbench menus and pickers. */
export const floatingSurfaceClass =
  'border-0 bg-[var(--floating-surface)] text-tx-0 shadow-popup';

export const floatingMenuContentClass =
  'z-50 min-w-[132px] overflow-hidden rounded-md border-0 bg-[var(--floating-surface)] p-[4px] font-sans text-tx-0 shadow-popup';

export const floatingMenuItemClass =
  'relative flex min-h-[44px] cursor-pointer select-none items-center gap-[8px] rounded-sm px-[10px] py-0 font-sans text-xs text-tx-1 outline-none transition-colors duration-fast focus:bg-[var(--floating-item-hover)] focus:text-tx-0 data-[highlighted]:bg-[var(--floating-item-hover)] data-[highlighted]:text-tx-0 data-[disabled]:cursor-not-allowed data-[disabled]:text-tx-3 data-[disabled]:opacity-60 sm:min-h-[30px]';

export const floatingMenuSelectedClass =
  'data-[state=checked]:bg-[var(--floating-item-selected)] data-[state=checked]:font-strong data-[state=checked]:text-indigo-soft';

export const floatingMenuLabelClass =
  'px-[10px] py-[6px] font-sans text-[11px] font-strong text-tx-3';

/** Separators are whitespace in the workbench menu language, not rules. */
export const floatingMenuSeparatorClass = 'my-[2px] h-[4px] bg-transparent';
