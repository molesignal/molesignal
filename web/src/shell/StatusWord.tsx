import type * as React from 'react';

import { cn } from '@/shell/lib/cn';

export type StatusTone = 'neutral' | 'dim' | 'indigo' | 'blue' | 'green' | 'yellow' | 'orange' | 'red' | 'purple';

const INK: Record<StatusTone, string> = {
  neutral: 'text-tx-1',
  dim: 'text-tx-3',
  indigo: 'text-[var(--indigo-ink)]',
  blue: 'text-[var(--blue-ink)]',
  green: 'text-[var(--green-ink)]',
  yellow: 'text-[var(--yellow-ink)]',
  orange: 'text-[var(--orange-ink)]',
  red: 'text-[var(--red-ink)]',
  purple: 'text-[var(--purple-ink)]',
};

/** The solid mark a state carries before its word, in the tone's full colour. */
const DOT: Partial<Record<StatusTone, string>> = {
  green: 'var(--green)',
  yellow: 'var(--yellow)',
  orange: 'var(--orange)',
  red: 'var(--red)',
};

/**
 * A status in a table or a list: the word in its ink, with nothing behind it, as
 * the trace table's HTTP method is. A chip in every row makes a dense table loud.
 * A state (green, yellow, orange, red) also takes a solid dot before it, so a
 * glance down a column finds what went wrong; categorical tones (blue, purple)
 * stay words. `strong` is for a state that wants attention: the normal one stays
 * at the body weight.
 */
export function StatusWord({
  tone = 'neutral',
  strong = false,
  dot,
  className,
  children,
}: {
  tone?: StatusTone | undefined;
  strong?: boolean | undefined;
  /** Leave the dot out where the state is already drawn beside the word. */
  dot?: boolean | undefined;
  className?: string | undefined;
  children: React.ReactNode;
}) {
  const mark = dot !== false ? DOT[tone] : undefined;

  return (
    <span
      className={cn(
        'whitespace-nowrap font-sans',
        mark && 'inline-flex items-center gap-1.5',
        INK[tone],
        strong && 'font-strong',
        className,
      )}
    >
      {mark && (
        <span aria-hidden data-status-dot className="h-[6px] w-[6px] shrink-0 rounded-full" style={{ background: mark }} />
      )}
      {children}
    </span>
  );
}
