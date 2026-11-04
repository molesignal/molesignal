import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { formatClock } from '../../replay/clock';

/** The room a list has before it scrolls. */
const LIST_HEIGHT = 'max-h-[300px]';

/** A row of a list, the head that stays over the rows, and the row the replay is at (a line on its left, as a place in a list is marked). */
export const ROW = 'relative grid min-h-8 items-center gap-3 border-b border-[var(--row-rule)] px-3 py-[5px] font-sans text-xs';
export const HEAD = 'sticky top-0 z-10 grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0';
export const LIT = 'bg-[var(--floating-item-selected)] shadow-[inset_2px_0_0_var(--blue)]';

/** A list longer than this is drawn row by row as it scrolls into view. */
export const LONG_LIST = 200;

/** The rows of a list: it scrolls by itself and keeps its head in sight; the page does not move with it. */
export const ListScroller = React.forwardRef<HTMLDivElement, { label: string; minWidth: number; children: React.ReactNode }>(function ListScroller(
  { label, minWidth, children },
  ref,
) {
  return (
    <div ref={ref} data-rum-tool-list="" className={cn(LIST_HEIGHT, 'overflow-auto')}>
      <div role="table" aria-label={label} style={{ minWidth }}>
        {children}
      </div>
    </div>
  );
});

/** What a row looks like to drawing: a long list leaves the rows out of sight undrawn. */
export const rowClass = (lit: boolean, long: boolean): string =>
  cn(ROW, lit && LIT, long && '[contain-intrinsic-size:auto_32px] [content-visibility:auto]');

/**
 * When a row happened, on the ruler of the replay: minutes and seconds from its beginning. Where a replay
 * can be moved the time is the way to move it to that moment, and its button stretches over the whole row.
 */
export function Moment({
  at,
  zero,
  seekable,
  label,
  onSeek,
}: {
  /** When the row happened, and where the ruler starts, in microseconds since the epoch. */
  at: number;
  zero: number;
  seekable: boolean;
  label: string;
  onSeek: () => void;
}) {
  const time = formatClock((at - zero) / 1_000, true);

  if (!seekable) {
    return (
      <span role="cell" className="font-mono text-tx-3">
        {time}
      </span>
    );
  }

  return (
    <span role="cell" className="min-w-0">
      <button
        type="button"
        onClick={onSeek}
        aria-label={label}
        data-rum-seek=""
        className="border-0 bg-transparent p-0 font-mono text-xs text-tx-2 outline-none hover:text-tx-0 focus-visible:text-tx-0 after:absolute after:inset-0 after:content-['']"
      >
        {time}
      </button>
    </span>
  );
}

/** The words a list says when it has no row. */
export function NoRows({ children }: { children: React.ReactNode }) {
  return (
    <p data-rum-tool-empty="" className="m-0 px-3 py-6 text-center font-sans text-xs text-tx-3">
      {children}
    </p>
  );
}
