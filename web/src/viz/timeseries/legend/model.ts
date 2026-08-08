import type { TimeSeriesSeries, TimeSeriesStats } from '../types';

export interface TimeSeriesLegendSeries extends TimeSeriesSeries {
  id: string;
  color: string;
}

export type TimeSeriesLegendSelectionMode = 'isolate' | 'append';

export interface LegendRow {
  series: TimeSeriesLegendSeries;
  stats: TimeSeriesStats;
}
