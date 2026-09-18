import { cn } from '@/shell/lib/cn';

/**
 * Token-aware Skeleton placeholder block.
 *
 * Uses bg-bg-3 over the surface so placeholders remain part of the same UI
 * hierarchy while interactive emphasis stays distinct.
 *
 * The `animate-pulse` keyframe is silenced under prefers-reduced-motion
 * via the global rule in tokens.css.
 */
function Skeleton({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('animate-pulse rounded-md bg-bg-3', className)} {...props} />;
}

export { Skeleton };
