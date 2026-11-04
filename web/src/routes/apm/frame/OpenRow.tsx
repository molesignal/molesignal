import { Star } from 'lucide-react';
import type * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { isNestedControlClick } from '@/shell/lib/nestedControl';
import { springPop } from '@/shell/polish/springPop';

/**
 * A row of an APM table that opens what it lists, on a click or on Enter or
 * Space while it has the focus. A control inside it (a star) keeps its own
 * click: the row hears it and leaves it be.
 */
export function OpenRow({ onOpen, children, ...props }: React.HTMLAttributes<HTMLDivElement> & { onOpen: () => void }) {
  return (
    <div
      role="row"
      tabIndex={0}
      {...props}
      onClick={(event) => {
        if (!isNestedControlClick(event)) onOpen();
      }}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget || (event.key !== 'Enter' && event.key !== ' ')) return;
        event.preventDefault();
        onOpen();
      }}
    >
      {children}
    </div>
  );
}

/**
 * A star that marks what matters to the reader or the team: a favorite
 * service, a key transaction. Filled in yellow while on; dim and filled
 * where the reader may see it but not change it.
 */
export function StarToggle({
  on,
  label,
  onToggle,
  disabled = false,
  title,
  className,
}: {
  on: boolean;
  label: string;
  onToggle: () => void;
  disabled?: boolean;
  title?: string | undefined;
  className?: string;
}) {
  return (
    <button
      type="button"
      aria-pressed={on}
      aria-label={label}
      title={title ?? label}
      disabled={disabled}
      onClick={(event) => {
        if (!on) springPop(event.currentTarget);
        onToggle();
      }}
      data-apm-star={on ? 'on' : 'off'}
      className={cn(
        'relative flex h-6 w-6 shrink-0 items-center justify-center rounded-sm after:absolute after:-inset-1 after:content-[""] focus-visible:outline-none enabled:hover:bg-bg-3 enabled:focus-visible:bg-bg-3 disabled:cursor-default',
        on ? 'text-[var(--yellow-ink)]' : 'text-tx-4 enabled:hover:text-tx-2',
        className,
      )}
    >
      <Star aria-hidden className="h-3.5 w-3.5" fill={on ? 'currentColor' : 'none'} />
    </button>
  );
}
