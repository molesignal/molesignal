import { cn } from '@/shell/lib/cn';
import { Skeleton } from '@/shell/ui/skeleton';

// Fixed so the placeholder does not shimmer into a different shape each render.
const PLOT_BARS = [38, 52, 44, 66, 58, 72, 49, 61, 80, 55, 68, 74, 47, 63, 57, 78, 52, 69, 60, 45, 71, 56, 64, 50];

/** Placeholder with the same frame as the intake chart: header figure, then bars on a baseline. */
export function PlotSkeleton({ className }: { className?: string }) {
  return (
    <div
      role="status"
      aria-busy="true"
      className={cn('flex h-full min-h-0 flex-col px-[20px] pb-[18px] pt-3', className)}
    >
      <Skeleton className="h-3.5 w-24" />
      <Skeleton className="mt-3 h-8 w-32" />
      <div className="mt-4 flex min-h-[120px] flex-1 items-end gap-[6px] border-b border-bd-0 pb-0">
        {PLOT_BARS.map((height, index) => (
          <Skeleton
            key={index}
            className="flex-1 rounded-b-none rounded-t-sm"
            style={{ height: `${height}%` }}
          />
        ))}
      </div>
    </div>
  );
}

/** Rows shaped like the streams table: name, type tag, status, then a short tail. */
export function TableSkeleton({ rows = 6, className }: { rows?: number; className?: string }) {
  return (
    <div role="status" aria-busy="true" className={cn('min-h-0 flex-1', className)}>
      <div className="flex h-9 items-center gap-6 px-[20px]">
        <Skeleton className="h-3 w-24" />
        <Skeleton className="h-3 w-12" />
        <Skeleton className="h-3 w-12" />
      </div>
      {Array.from({ length: rows }, (_, index) => (
        <div
          key={index}
          className="flex h-[var(--row-height)] items-center gap-6 border-t border-bd-0 px-[20px]"
        >
          <Skeleton className="h-3.5 w-40" />
          <Skeleton className="h-[22px] w-16 rounded-md" />
          <Skeleton className="h-[22px] w-14 rounded-full" />
          <Skeleton className="ml-auto h-3 w-16" />
        </div>
      ))}
    </div>
  );
}

/** A short list of two-line items, e.g. recent activity. */
export function ListSkeleton({ rows = 4, className }: { rows?: number; className?: string }) {
  return (
    <div
      role="status"
      aria-busy="true"
      className={cn('flex flex-col gap-4 px-[20px] py-3', className)}
    >
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex items-start gap-3">
          <Skeleton className="mt-1 h-2 w-2 rounded-full" />
          <div className="min-w-0 flex-1">
            <Skeleton className="h-3.5 w-3/5" />
            <Skeleton className="mt-2 h-3 w-4/5" />
          </div>
        </div>
      ))}
    </div>
  );
}
