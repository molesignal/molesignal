import { lazy, Suspense, type ComponentType } from 'react';

import type { Frame, FrameKind } from '@/stores/useInvestigationStack';

export interface FrameProps {
  frame: Frame;
}

/**
 * Lazy frame component registry. Each FrameKind maps to a code-split module so
 * the shell never imports a viz it doesn't need.
 */
const LOADERS: Record<FrameKind, ComponentType<FrameProps>> = {
  trace: lazy(() => import('./frames/trace/Frame').then((m) => ({ default: m.TraceFrame }))),
  trace_span: lazy(() => import('./frames/trace/SpanFrame').then((m) => ({ default: m.TraceSpanFrame }))),
  log: lazy(() => import('./frames/log/Frame').then((m) => ({ default: m.LogFrame }))),
  log_row: lazy(() => import('./frames/log/RowFrame').then((m) => ({ default: m.LogRowFrame }))),
  metric: lazy(() => import('./frames/MetricFrame').then((m) => ({ default: m.MetricFrame }))),
  host: lazy(() => import('./frames/HostFrame').then((m) => ({ default: m.HostFrame }))),
  service: lazy(() => import('./frames/service/Frame').then((m) => ({ default: m.ServiceFrame }))),
  service_to_service: lazy(() =>
    import('./frames/service/ToServiceFrame').then((m) => ({ default: m.ServiceToServiceFrame })),
  ),
  incident: lazy(() => import('./frames/IncidentFrame').then((m) => ({ default: m.IncidentFrame }))),
  sql: lazy(() => import('./frames/SqlFrame').then((m) => ({ default: m.SqlFrame }))),
  promql: lazy(() => import('./frames/PromqlFrame').then((m) => ({ default: m.PromqlFrame }))),
  dashboard_panel: lazy(() =>
    import('./frames/DashboardPanelFrame').then((m) => ({ default: m.DashboardPanelFrame })),
  ),
  saved_view: lazy(() => import('./frames/SavedViewFrame').then((m) => ({ default: m.SavedViewFrame }))),
};

export function FrameRenderer({ frame }: { frame: Frame }) {
  const Body = LOADERS[frame.kind];
  return (
    <Suspense fallback={<div className="p-5 text-xs text-muted-foreground">Loading…</div>}>
      <Body frame={frame} />
    </Suspense>
  );
}

export const FRAME_KIND_LABELS: Record<FrameKind, string> = {
  trace: 'Trace',
  trace_span: 'Span',
  log: 'Logs',
  log_row: 'Log row',
  metric: 'Metric',
  host: 'Host',
  service: 'Service',
  service_to_service: 'Service ↔ Service',
  incident: 'Incident',
  sql: 'SQL',
  promql: 'PromQL',
  dashboard_panel: 'Panel',
  saved_view: 'Saved view',
};
