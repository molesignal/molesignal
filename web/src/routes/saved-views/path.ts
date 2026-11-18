import type { SavedView } from '@/api/savedViews';

/** A look-back in whole days, hours or minutes, as the `?time=` window says it. */
function lookBack(seconds: number): string {
  for (const [unit, size] of [
    ['d', 86_400],
    ['h', 3_600],
    ['m', 60],
  ] as const) {
    if (seconds >= size && seconds % size === 0) return `${seconds / size}${unit}`;
  }
  return `${Math.max(1, Math.round(seconds / 60))}m`;
}

/**
 * Where a saved view opens: its statement in Metrics or Logs, its window counted back from whenever
 * it is opened.
 */
export function savedViewPath(
  view: Pick<SavedView, 'language' | 'statement' | 'stream' | 'timeRangeSecs'>,
): string {
  const params = new URLSearchParams({ time: `now-${lookBack(view.timeRangeSecs)}..now` });
  if (view.language === 'promql') {
    params.set('promql', view.statement);
    return `/metrics?${params.toString()}`;
  }
  params.set('sql', view.statement);
  if (view.stream) params.set('stream', view.stream);
  return `/logs?${params.toString()}`;
}
