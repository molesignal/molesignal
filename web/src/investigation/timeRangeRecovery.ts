import {
  resolveWindow,
  type TimeWindow,
} from '@/stores/useTimeStore';

/**
 * Expand a query window around its current end while preserving a stable,
 * shareable absolute range. Recovery actions use the same calculation across
 * Logs, Metrics, and Traces so "expand time" never means something different
 * between signals.
 */
export function widenTimeWindow(
  window: TimeWindow,
  factor = 4,
  now = new Date(),
): TimeWindow {
  const resolved = resolveWindow(window, now);
  const toMs = resolved.to.getTime();
  const durationMs = Math.max(60_000, toMs - resolved.from.getTime());
  return {
    mode: 'absolute',
    from: new Date(toMs - durationMs * Math.max(1, factor)).toISOString(),
    to: resolved.to.toISOString(),
  };
}
