import type * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { springPop } from '@/shell/polish/springPop';

/**
 * A filter that is on or off, coloured while on. `spring` is for a toggle that marks something
 * as the reader's own (a favourite): turning it on springs its icon.
 */
export function FilterToggle({
  pressed,
  onToggle,
  icon,
  children,
  spring = false,
  ...data
}: {
  pressed: boolean;
  onToggle: () => void;
  icon: React.ReactNode;
  children: React.ReactNode;
  spring?: boolean;
} & Record<`data-${string}`, string>) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      onClick={(event) => {
        if (spring && !pressed) springPop(event.currentTarget);
        onToggle();
      }}
      {...data}
      className={cn(
        'inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md px-2.5 font-sans text-xs font-strong outline-none transition-colors',
        pressed ? 'bg-indigo-dim text-indigo-soft' : 'bg-[var(--control-surface)] text-tx-1 hover:bg-bg-3 focus-visible:bg-bg-3',
      )}
    >
      {icon}
      {children}
    </button>
  );
}
