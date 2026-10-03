import { ChevronRight } from 'lucide-react';
import type * as React from 'react';

import { AnimatedNumber } from '@/shell/AnimatedNumber';
import { cn } from '@/shell/lib/cn';
import { Skeleton } from '@/shell/ui/skeleton';

import { splitMetricValue } from './format';

export type MetricTone = 'red' | 'yellow' | 'green';

const TONE_TEXT: Record<MetricTone, string> = {
  red: 'text-red',
  yellow: 'text-yellow',
  green: 'text-green',
};

/**
 * A secondary readout on the Home strip. The figure is the card; the unit and
 * the detail line stay quiet. State shows up in the figure's color, not in a
 * decorative badge.
 */
export function MetricCard({
  label,
  scope,
  value,
  detail,
  tone,
  loading = false,
  riseIndex = 0,
  onClick,
}: {
  label: string;
  /** Time scope the figure covers, so a reader never has to guess the window. */
  scope?: string | undefined;
  /** Already formatted, e.g. "1.2 GiB", "12", "—". */
  value: string;
  detail: React.ReactNode;
  tone?: MetricTone | undefined;
  loading?: boolean | undefined;
  riseIndex?: number | undefined;
  onClick: () => void;
}) {
  const { number, unit } = splitMetricValue(value);
  const numeric = Number(number.replace(/,/g, ''));
  const decimals = number.includes('.') ? (number.split('.')[1]?.length ?? 0) : 0;

  return (
    <button
      type="button"
      onClick={onClick}
      data-home-surface="kpi"
      aria-busy={loading || undefined}
      style={{ '--rise-index': riseIndex } as React.CSSProperties}
      className="home-canvas-metric home-rise group relative flex min-w-0 flex-col justify-between rounded-md bg-[var(--functional-surface)] px-[20px] py-[14px] text-left transition-colors duration-fast [box-shadow:var(--shadow-functional-surface)] hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:text-tx-0 focus-visible:outline-none"
    >
      <div className="flex min-w-0 items-baseline justify-between gap-2">
        <span className="type-label min-w-0 truncate font-medium text-tx-2">{label}</span>
        {scope && <span className="type-micro shrink-0 text-tx-3">{scope}</span>}
      </div>
      <div className="mt-2 min-w-0">
        {loading ? (
          <>
            <Skeleton className="h-8 w-24" />
            <Skeleton className="mt-2 h-3.5 w-36" />
          </>
        ) : (
          <>
            <div
              className={cn(
                'type-kpi flex items-baseline gap-1.5 font-sans font-display tracking-[-0.02em] tabular-nums',
                tone ? TONE_TEXT[tone] : 'text-tx-0',
              )}
            >
              {Number.isFinite(numeric) ? (
                <AnimatedNumber value={numeric} decimals={decimals} />
              ) : (
                <span>{number}</span>
              )}
              {unit && (
                <span className="type-data font-medium tracking-normal text-tx-2">{unit}</span>
              )}
            </div>
            <div className="type-caption mt-1.5 text-pretty text-tx-2">{detail}</div>
          </>
        )}
      </div>
      <ChevronRight
        aria-hidden="true"
        className="absolute bottom-[14px] right-[20px] h-3.5 w-3.5 translate-x-1 text-tx-3 opacity-0 transition-[opacity,transform] duration-fast group-hover:translate-x-0 group-hover:opacity-100 group-focus-visible:translate-x-0 group-focus-visible:opacity-100"
      />
    </button>
  );
}
