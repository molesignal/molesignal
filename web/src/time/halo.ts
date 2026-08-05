import { resolveExpr, type TimeWindow } from '@/stores/useTimeStore';

export type HaloKind = 'trace_span' | 'log_row' | 'metric_sample';

const HALO_MS: Record<HaloKind, number> = {
  trace_span: 30_000,
  log_row: 5_000,
  metric_sample: 60_000,
};

/**
 * Build a TimeWindow centered on `at` with a kind-specific halo, intersected
 * with the global window. Returned as absolute strings ready for URL/API.
 */
export function halo(kind: HaloKind, at: string, global: TimeWindow): TimeWindow {
  const center = new Date(at);
  const margin = HALO_MS[kind];
  const from = new Date(center.getTime() - margin);
  const to = new Date(center.getTime() + margin);

  const now = new Date();
  const globalFrom = resolveExpr(global.from, now);
  const globalTo = resolveExpr(global.to, now);

  const clampedFrom = from < globalFrom ? globalFrom : from;
  const clampedTo = to > globalTo ? globalTo : to;

  return { from: clampedFrom.toISOString(), to: clampedTo.toISOString(), mode: 'absolute' };
}
