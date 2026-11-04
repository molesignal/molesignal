import { Link } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';

import { formatCount, formatPercent } from './format';

export interface BarRow {
  key: string;
  label: string;
  /** What the bar is as long as; a count unless the row says how it is written. */
  count: number;
  /** How the number is written when it is not a count (`1.4 s`); such a row has no share of a total. */
  text?: string | undefined;
  /** The colour and weight of the number, where it says something (a vital that is poor). */
  textClassName?: string | undefined;
  /** Where the row goes, to look at what it counts; the rest of a total goes nowhere. */
  to?: string | undefined;
  /** Picks the row as a filter of the page it is on; picked again, it lets go. */
  onSelect?: (() => void) | undefined;
  /** Whether the row is the one picked. */
  active?: boolean | undefined;
}

/**
 * One measure, spread over some values: each a bar as long as its count is next to the largest,
 * with the count and its share of the total after the bar, the way the other modules draw a
 * distribution. A row that has a place to go is a link; one that can be picked is a button.
 */
export function BarRows({
  rows,
  total,
  language,
  barClassName = 'bg-[var(--chart-1)]',
  labelWidth = 96,
  testId,
}: {
  rows: readonly BarRow[];
  total: number;
  language: string;
  barClassName?: string;
  /** How wide the names are, in px, before they are cut. */
  labelWidth?: number;
  testId: string;
}) {
  const widest = Math.max(1, ...rows.map((row) => row.count));

  return (
    <ol data-rum-bars={testId} className="space-y-0.5 px-3 py-2">
      {rows.map((row) => {
        const share = total > 0 ? row.count / total : 0;
        const counted = row.text === undefined;
        const label = row.onSelect ? (
          <button
            type="button"
            aria-pressed={row.active ?? false}
            onClick={row.onSelect}
            className={cn(
              'min-w-0 truncate border-0 bg-transparent p-0 text-right font-sans outline-none hover:text-tx-0 focus-visible:text-tx-0',
              row.active ? 'font-strong text-tx-0' : 'text-tx-1',
            )}
          >
            {row.label}
          </button>
        ) : row.to ? (
          <Link to={row.to} className="truncate text-right text-tx-1 outline-none hover:text-tx-0 focus-visible:text-tx-0">
            {row.label}
          </Link>
        ) : (
          <span className="truncate text-right text-tx-3">{row.label}</span>
        );

        return (
          <li
            key={row.key}
            data-rum-bar={row.key}
            style={{ gridTemplateColumns: `minmax(0,${labelWidth}px) minmax(0,1fr)` }}
            className="type-micro grid h-[18px] items-center gap-2 font-sans"
          >
            {label}
            <span className="relative mr-[72px] h-[10px]">
              <span
                className={cn('absolute inset-y-0 left-0 rounded-[3px]', barClassName)}
                style={{ width: `${Math.max(0.6, (row.count / widest) * 100)}%` }}
              />
              <span
                className="pointer-events-none absolute top-1/2 flex -translate-y-1/2 items-center gap-1.5 whitespace-nowrap pl-1.5 tabular-nums"
                style={{ left: `${(row.count / widest) * 100}%` }}
              >
                <span className={row.textClassName ?? 'text-tx-1'}>{counted ? formatCount(row.count, language) : row.text}</span>
                {counted && <span className="text-tx-3">{formatPercent(share)}</span>}
              </span>
            </span>
          </li>
        );
      })}
    </ol>
  );
}
