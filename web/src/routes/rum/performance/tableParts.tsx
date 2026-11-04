import { Search } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

/** The look of a filter or a choice in the head of a table. */
export const CONTROL = 'h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs hover:bg-bg-3 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3';

/** The head row of a table, and a row of its body: the columns are the grid the page gives them. */
export const HEAD_ROW =
  'grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0';

export const BODY_ROW = 'relative grid h-8 items-center gap-3 border-b border-[var(--row-rule)] px-3 font-sans text-xs hover:bg-bg-2 focus-within:bg-bg-2';

/** How long typing waits before the table is narrowed. */
const TYPING_MS = 250;

/**
 * Words the table is narrowed to, typed in a field. What the address says wins over what was typed
 * when it changes elsewhere; typing sends the words on once it has paused.
 */
export function useTypedSearch(search: string, onSearch: (words: string) => void) {
  const [draft, setDraft] = React.useState(search);

  React.useEffect(() => setDraft(search), [search]);

  React.useEffect(() => {
    if (draft === search) return undefined;

    const timer = globalThis.setTimeout(() => onSearch(draft), TYPING_MS);

    return () => globalThis.clearTimeout(timer);
  }, [draft, search, onSearch]);

  return [draft, setDraft] as const;
}

export function SearchField({
  value,
  onChange,
  placeholder,
  label,
  className,
}: {
  value: string;
  onChange: (words: string) => void;
  placeholder: string;
  label: string;
  className?: string | undefined;
}) {
  return (
    <label className="relative">
      <span className="sr-only">{label}</span>
      <Search aria-hidden className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-tx-3" />
      <input
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        className={cn(
          'h-8 w-60 rounded-md border-0 bg-[var(--control-surface)] pl-8 pr-2.5 font-sans text-xs font-body text-tx-1 outline-none placeholder:text-tx-3 hover:bg-bg-3 focus-visible:bg-bg-3',
          className,
        )}
      />
    </label>
  );
}

/** The heading of a column that orders the table: the way it is ordered says itself, and the column the page is of is lit. */
export function SortHeading({
  column,
  label,
  active,
  lit,
  direction,
  align = 'right',
  onSort,
}: {
  column: string;
  label: string;
  /** The table is ordered by this column. */
  active: boolean;
  /** The column stands out though it does not order the table. */
  lit?: boolean | undefined;
  direction: 'asc' | 'desc';
  align?: 'left' | 'right';
  onSort: () => void;
}) {
  return (
    <span
      role="columnheader"
      aria-sort={active ? (direction === 'asc' ? 'ascending' : 'descending') : 'none'}
      data-rum-column={column}
      className={cn(align === 'right' && 'text-right')}
    >
      <button
        type="button"
        onClick={onSort}
        className={cn(
          'inline-flex items-center gap-1 border-0 bg-transparent p-0 font-sans text-xs font-strong outline-none hover:text-tx-0 focus-visible:text-tx-0',
          active || lit ? 'text-tx-0' : 'text-tx-1',
        )}
      >
        {label}
        {active && <span aria-hidden>{direction === 'asc' ? '↑' : '↓'}</span>}
      </button>
    </span>
  );
}

/** The words a table says under its head when a search left it without a row. */
export function NoMatch({ children }: { children: React.ReactNode }) {
  return (
    <p data-rum-no-match="" className="m-0 px-3 py-6 text-center font-sans text-xs text-tx-3">
      {children}
    </p>
  );
}

/** The grade of a share of failures, in the ink it is told in: a hundredth is worth saying, a twentieth is a fault. */
export function failureInk(rate: number | null): string {
  if (rate === null) return 'text-tx-4';
  if (rate >= 0.05) return 'font-strong text-[var(--red-ink)]';
  if (rate >= 0.01) return 'font-strong text-[var(--orange-ink)]';

  return 'text-[var(--green-ink)]';
}

/** A share of failures with the decimal it needs: 2.1%, and `<0.1%` for what is there but small. */
export function formatRate(rate: number): string {
  if (!Number.isFinite(rate) || rate <= 0) return '0%';
  if (rate < 0.001) return '<0.1%';

  return `${Number((rate * 100).toFixed(1))}%`;
}
