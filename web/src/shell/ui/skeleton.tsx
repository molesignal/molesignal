import { cn } from '@/shell/lib/cn';

/**
 * Skeleton — Phase 4 token-aware placeholder block.
 *
 * Uses bg-bg-3 (the hover layer) over the surface so it reads as "an
 * empty piece of the same UI." The default shadcn `bg-primary/10` would
 * tint everything Indigo and lie about which surfaces are interactive.
 *
 * The `animate-pulse` keyframe is silenced under prefers-reduced-motion
 * via the global rule in tokens.css.
 */
function Skeleton({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('animate-pulse rounded-md bg-bg-3', className)} {...props} />;
}

export { Skeleton };
