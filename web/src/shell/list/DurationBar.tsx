import { cn } from '@/shell/lib/cn';

/**
 * How long a row of a list took: the time in figures, then a bar as long against its track as the
 * time is against the longest one listed, so the slow rows stand out down the column. Any time
 * shows a little bar. A row that failed has a red one.
 */
export function DurationBar({
  text,
  value,
  longest,
  failed = false,
}: {
  /** The time as it is read. */
  text: string;
  value: number;
  /** The longest time listed, in the unit of `value`. */
  longest: number;
  failed?: boolean;
}) {
  const share = longest > 0 ? Math.max(2, (value / longest) * 100) : 0;

  return (
    <span data-duration-bar="" className="flex min-w-0 flex-1 items-center gap-2">
      <span className="w-[58px] shrink-0 text-right tabular-nums text-tx-0">{text}</span>
      <span aria-hidden="true" className="relative h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-bg-3">
        <span
          className={cn('absolute inset-y-0 left-0 rounded-full', failed ? 'bg-red' : 'bg-[var(--chart-1)]')}
          style={{ width: `${share}%` }}
        />
      </span>
    </span>
  );
}
