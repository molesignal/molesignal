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

/**
 * A status in a table or a list: the word in its ink, with nothing behind it, as
 * the trace table's HTTP method is. A chip in every row makes a dense table loud.
 * `strong` is for a state that wants attention: the normal one stays at the body
 * weight.
 */
export function StatusWord({
  tone = 'neutral',
  strong = false,
  className,
  children,
}: {
  tone?: StatusTone | undefined;
  strong?: boolean | undefined;
  className?: string | undefined;
  children: React.ReactNode;
}) {
  return (
    <span className={cn('whitespace-nowrap font-sans', INK[tone], strong && 'font-strong', className)}>
      {children}
    </span>
  );
}
