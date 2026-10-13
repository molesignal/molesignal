import type * as React from 'react';

import { useEasedNumber } from '@/shell/AnimatedNumber';
import { cn } from '@/shell/lib/cn';

import { Sparkline } from './Sparkline';
import { readoutNumber, splitReadoutValue } from './value';

export type ReadoutTone = 'good' | 'warn' | 'danger';

const TONE_TEXT: Record<ReadoutTone, string> = {
  good: 'text-green',
  warn: 'text-yellow',
  danger: 'text-red',
};

export interface ReadoutTrend {
  values: ReadonlyArray<number | null | undefined>;
  variant?: 'line' | 'bars';
  zeroBased?: boolean;
}

/**
 * One figure of a page's opening statement: what it is, the number, the shape
 * it has had over the window, and a line of context.
 *
 * It is a cell, not a card: several sit side by side on one surface, so a row
 * of them reads as one instrument instead of a grid of boxes. The figure counts
 * up when it arrives and its state is in its colour, not in a badge.
 */
export function Readout({
  label,
  value,
  detail,
  tone,
  trend,
  trendClassName = 'text-indigo-soft',
  riseIndex = 0,
  className,
}: {
  label: React.ReactNode;
  /** Already formatted: "46.9 req/s", "1.12%", "420 ms", "—". */
  value: string;
  detail?: React.ReactNode;
  tone?: ReadoutTone | undefined;
  trend?: ReadoutTrend | undefined;
  /** The colour of the trend, as a text colour; it follows the tone when there is one. */
  trendClassName?: string | undefined;
  riseIndex?: number | undefined;
  className?: string | undefined;
}) {
  const { number, unit } = splitReadoutValue(value);

  return (
    <div
      data-readout
      style={{ '--rise-index': riseIndex } as React.CSSProperties}
      className={cn('ms-rise flex min-w-0 flex-col', className)}
    >
      <div className="type-label truncate font-medium text-tx-2">{label}</div>
      <div
        className={cn(
          'type-kpi mt-1.5 flex min-w-0 items-baseline gap-1.5 font-sans font-display tracking-[-0.02em] tabular-nums',
          tone ? TONE_TEXT[tone] : 'text-tx-0',
        )}
      >
        <ReadoutFigure number={number} />
        {unit && (
          <span className="type-data shrink-0 font-medium tracking-normal text-tx-2">{unit}</span>
        )}
      </div>
      {trend && (
        <Sparkline
          values={trend.values}
          variant={trend.variant ?? 'line'}
          zeroBased={trend.zeroBased ?? false}
          className={cn('mt-2', tone ? TONE_TEXT[tone] : trendClassName)}
        />
      )}
      {detail && <div className="type-caption mt-1.5 truncate text-tx-2">{detail}</div>}
    </div>
  );
}

function ReadoutFigure({ number }: { number: string }) {
  const parsed = readoutNumber(number);
  if (!parsed) return <span className="truncate">{number}</span>;
  return <CountedFigure {...parsed} />;
}

function CountedFigure({
  value,
  decimals,
  grouped,
}: {
  value: number;
  decimals: number;
  grouped: boolean;
}) {
  const eased = useEasedNumber(value);
  return (
    <span className="truncate">
      {eased.toLocaleString('en-US', {
        minimumFractionDigits: decimals,
        maximumFractionDigits: decimals,
        useGrouping: grouped,
      })}
    </span>
  );
}
