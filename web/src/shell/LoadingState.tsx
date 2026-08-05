import { CircleStop } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

/**
 * LoadingState — brief Principle "Empty/Error/Loading 都是头等公民".
 *
 * Skeletons match the real layout (not a generic shimmer of rectangles).
 * After 3 seconds the strip surfaces an `elapsed` timer plus a Cancel
 * button. SREs at 3am don't want a spinner that lies to them.
 *
 * `prefers-reduced-motion` is honored by tokens.css — the pulse becomes
 * a static state, the timer continues to tick.
 */

type Variant = 'query' | 'list' | 'chart';

interface LoadingStateProps {
  variant?: Variant | undefined;
  /** Number of placeholder rows for the `list` variant. */
  rows?: number | undefined;
  /** Optional cancel hook. When supplied, a Cancel button appears after
   *  3 seconds with the elapsed time. */
  onCancel?: (() => void) | undefined;
  /** Override the elapsed-timer onset (ms). Default 3000. */
  showCancelAfterMs?: number | undefined;
  className?: string | undefined;
  'data-testid'?: string | undefined;
}

export function LoadingState({
  variant = 'list',
  rows = 6,
  onCancel,
  showCancelAfterMs = 3000,
  className,
  'data-testid': testId,
}: LoadingStateProps) {
  const elapsed = useElapsedMs(showCancelAfterMs);
  const showCancel = onCancel && elapsed !== null;

  return (
    <div
      role="status"
      aria-live="polite"
      aria-busy="true"
      data-testid={testId}
      data-variant={variant}
      className={cn('flex w-full flex-col gap-3', className)}
    >
      {variant === 'query' && <QuerySkeleton />}
      {variant === 'list' && <ListSkeleton rows={rows} />}
      {variant === 'chart' && <ChartSkeleton />}

      {showCancel && (
        <div className="flex items-center justify-center gap-2 pt-2 font-sans text-xs text-tx-2">
          <span className="tabular-nums" aria-label="elapsed time">
            {formatElapsed(elapsed!)}
          </span>
          <button
            type="button"
            onClick={onCancel}
            className={cn(
              'inline-flex h-8 items-center gap-1.5 rounded-md border border-bd-1 bg-bg-2 px-2.5 font-strong text-tx-1',
              'transition-colors duration-fast ease-default',
              'hover:bg-bg-3 hover:text-tx-0',
              'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
            )}
            data-testid="loading-cancel"
          >
            <CircleStop className="h-3 w-3" />
            <span>Cancel</span>
          </button>
        </div>
      )}
    </div>
  );
}

/* --- variants ------------------------------------------------------ */

function QuerySkeleton() {
  return (
    <>
      {/* query bar */}
      <div className="flex items-center gap-2">
        <SkeletonBlock className="h-8 flex-1 rounded-md" />
        <SkeletonBlock className="h-8 w-24 rounded-md" />
      </div>
      {/* histogram strip */}
      <SkeletonBlock className="h-14 rounded-md" />
      {/* result header */}
      <div className="grid grid-cols-[200px_1fr] gap-3">
        <SkeletonBlock className="h-4 rounded" />
        <SkeletonBlock className="h-4 rounded" />
      </div>
    </>
  );
}

function ListSkeleton({ rows }: { rows: number }) {
  return (
    <div className="flex flex-col gap-1.5">
      {/* table head */}
      <div className="grid grid-cols-[24px_1fr_120px_80px] gap-3 px-3 py-1.5">
        <SkeletonBlock className="h-3 rounded" />
        <SkeletonBlock className="h-3 rounded" />
        <SkeletonBlock className="h-3 rounded" />
        <SkeletonBlock className="h-3 rounded" />
      </div>
      {Array.from({ length: rows }).map((_, i) => (
        <div
          key={i}
          className="grid grid-cols-[24px_1fr_120px_80px] gap-3 px-3 py-1.5"
          style={{ opacity: 1 - i * 0.08 }}
        >
          <SkeletonBlock className="h-4 w-4 rounded-full" />
          <SkeletonBlock className="h-4 rounded" />
          <SkeletonBlock className="h-4 rounded" />
          <SkeletonBlock className="h-4 rounded" />
        </div>
      ))}
    </div>
  );
}

function ChartSkeleton() {
  return (
    <div className="flex flex-col gap-2">
      {/* chart canvas */}
      <SkeletonBlock className="h-44 rounded-md" />
      {/* x-axis ticks */}
      <div className="grid grid-cols-6 gap-2 px-2">
        {Array.from({ length: 6 }).map((_, i) => (
          <SkeletonBlock key={i} className="h-3 rounded" />
        ))}
      </div>
    </div>
  );
}

function SkeletonBlock({ className }: { className?: string }) {
  return <div aria-hidden className={cn('animate-pulse bg-bg-3', className)} />;
}

/* --- helpers ------------------------------------------------------- */

function useElapsedMs(thresholdMs: number): number | null {
  const [elapsed, setElapsed] = React.useState<number | null>(null);
  React.useEffect(() => {
    const start = Date.now();
    let intervalId: number | undefined;
    const timeoutId = window.setTimeout(() => {
      setElapsed(Date.now() - start);
      intervalId = window.setInterval(() => setElapsed(Date.now() - start), 250);
    }, thresholdMs);
    return () => {
      window.clearTimeout(timeoutId);
      if (intervalId !== undefined) window.clearInterval(intervalId);
    };
  }, [thresholdMs]);
  return elapsed;
}

function formatElapsed(ms: number): string {
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(1)}s elapsed`;
  const m = Math.floor(s / 60);
  const rem = Math.floor(s % 60);
  return `${m}m ${rem}s elapsed`;
}
