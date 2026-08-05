import type * as React from 'react';

import type {
  MetricSeries,
  MetricSeriesQuality,
} from '@/lib/metricsSeries';
import type { QueryResult } from '@/types/query';
import type { TimeSeriesSeries } from '@/viz/timeseries/types';

import type { ExemplarRail } from '../ExemplarRail';
import type { MetricsDrawStyle, MetricsStackMode } from '../model';

export interface MetricsExploreResultsProps {
  query: {
    result: QueryResult | undefined;
    error: unknown;
    pending: boolean;
    promql: string;
    executedPromql: string | null;
    chartTitle: string;
  };
  series: {
    metricSeries: MetricSeries[];
    chartSeries: TimeSeriesSeries[];
    quality: MetricSeriesQuality;
    unit?: string;
    counterRateQuery: boolean;
  };
  chart: {
    xDomain: [number, number];
    timezone: string;
    drawStyle: MetricsDrawStyle;
    stackMode: MetricsStackMode;
    zoomed: boolean;
    onDrawStyleChange: (style: MetricsDrawStyle) => void;
    onStackModeChange: (mode: MetricsStackMode) => void;
    onRangeSelect: (range: { from: number; to: number }) => void;
    onRangeReset: () => void;
  };
  exemplars: {
    series: React.ComponentProps<typeof ExemplarRail>['series'];
    warning?: string;
    error?: string;
  };
  timeRangeSeconds: number;
  language: string;
  preferredView: 'graph' | 'table';
  onPreferredViewChange: (view: 'graph' | 'table') => void;
  onViewRawCounter: () => void;
  onInspectMetricType: () => void;
}

export interface GraphViewProps {
  query: MetricsExploreResultsProps['query'];
  series: MetricsExploreResultsProps['series'];
  chart: MetricsExploreResultsProps['chart'];
  exemplars: MetricsExploreResultsProps['exemplars'];
  onViewRawCounter: () => void;
  onInspectMetricType: () => void;
}
