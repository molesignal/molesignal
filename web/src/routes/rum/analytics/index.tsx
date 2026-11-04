import * as React from 'react';

import { FunnelView } from './FunnelView';
import { HeatmapView } from './heatmap/HeatmapView';
import { PathsView } from './PathsView';
import { RetentionView } from './RetentionView';
import { SegmentSelect, useSegmentFilters } from './segment';
import { AnalyticsViews } from './Views';
import { RUM_WEEK_WINDOW } from '../explorer/timeWindow';
import { type ScopeContext, ScopedFrame } from '../ScopedFrame';

/** The view with what it reads narrowed to the segment the address names. */
function Segmented({ context, children }: { context: ScopeContext; children: (context: ScopeContext) => React.ReactNode }) {
  const filters = useSegmentFilters(context.application);
  const scope = React.useMemo(() => (filters.length > 0 ? { ...context.scope, filters } : context.scope), [context.scope, filters]);

  return <>{children({ ...context, scope })}</>;
}

/**
 * The analytics tab: questions about what people do across a session. Its first view is the funnel;
 * the others follow the pages sessions went through and count who came back. Each can be of one
 * segment of the application.
 */
function AnalyticsFrame({ children }: { children: (context: ScopeContext) => React.ReactNode }) {
  return (
    <ScopedFrame
      name="analytics"
      opensOn={RUM_WEEK_WINDOW}
      views={(params) => <AnalyticsViews params={params} />}
      toolbar={(context) => <SegmentSelect application={context.application} />}
    >
      {(context) => <Segmented context={context}>{children}</Segmented>}
    </ScopedFrame>
  );
}

export function RumAnalytics() {
  return <AnalyticsFrame>{(context) => <FunnelView context={context} />}</AnalyticsFrame>;
}

export function RumAnalyticsPaths() {
  return <AnalyticsFrame>{(context) => <PathsView context={context} />}</AnalyticsFrame>;
}

export function RumAnalyticsHeatmaps() {
  return <AnalyticsFrame>{(context) => <HeatmapView context={context} />}</AnalyticsFrame>;
}

export function RumAnalyticsRetention() {
  return <AnalyticsFrame>{(context) => <RetentionView context={context} />}</AnalyticsFrame>;
}
