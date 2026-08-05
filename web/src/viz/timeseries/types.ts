export type TimeSeriesDrawStyle = 'line' | 'area' | 'bar' | 'points';
export type TimeSeriesInterpolation = 'linear' | 'stepBefore' | 'stepAfter';
export type TimeSeriesStackMode = 'none' | 'normal' | 'percent';
export type TimeSeriesPointMode = 'auto' | 'always' | 'never';
export type TimeSeriesTooltipMode = 'single' | 'all' | 'hidden';
export type TimeSeriesTooltipSort = 'none' | 'asc' | 'desc';
export type TimeSeriesLegendMode = 'list' | 'table' | 'hidden';
export type TimeSeriesLegendPlacement = 'bottom' | 'right';
export type TimeSeriesLegendStat = 'last' | 'min' | 'max' | 'mean' | 'sum';
export type TimeSeriesScale = 'linear' | 'log2' | 'log10' | 'symlog';

export interface TimeSeriesSeries {
  /** Stable identity used for color assignment and legend state. */
  id?: string;
  name: string;
  data: ReadonlyArray<number | null>;
  /**
   * Epoch timestamps. Seconds, milliseconds and microseconds are detected
   * automatically. When omitted, point indexes are used.
   */
  timestamps?: ReadonlyArray<number>;
  labels?: Readonly<Record<string, string>>;
  color?: string;
  unit?: string;
  axis?: 'left' | 'right';
  lineWidth?: number;
  dash?: ReadonlyArray<number>;
}

export interface TimeSeriesAxisOptions {
  scale: TimeSeriesScale;
  label?: string;
  unit?: string;
  min?: number;
  max?: number;
  softMin?: number;
  softMax?: number;
  showGrid?: boolean;
}

export interface TimeSeriesThreshold {
  value: number;
  label?: string;
  color?: string;
  showLine?: boolean;
}

export interface TimeSeriesBand {
  from?: number;
  to?: number;
  color: string;
}

export interface TimeSeriesAnnotation {
  id: string;
  timestamp: number;
  label: string;
  color?: string;
  endTimestamp?: number;
}

export interface TimeSeriesChartOptions {
  drawStyle: TimeSeriesDrawStyle;
  interpolation: TimeSeriesInterpolation;
  lineWidth: number;
  fillOpacity: number;
  showPoints: TimeSeriesPointMode;
  stackMode: TimeSeriesStackMode;
  connectNulls: boolean;
  tooltipMode: TimeSeriesTooltipMode;
  tooltipSort: TimeSeriesTooltipSort;
  tooltipHideZeros: boolean;
  tooltipMaxItems: number;
  legendMode: TimeSeriesLegendMode;
  legendPlacement: TimeSeriesLegendPlacement;
  legendStats: TimeSeriesLegendStat[];
  showXAxis: boolean;
  showYAxis: boolean;
  compactAxes: boolean;
  leftAxis: TimeSeriesAxisOptions;
  rightAxis?: TimeSeriesAxisOptions;
  thresholds: TimeSeriesThreshold[];
  bands: TimeSeriesBand[];
  annotations: TimeSeriesAnnotation[];
}

export type TimeSeriesChartOptionsInput = Omit<
  Partial<TimeSeriesChartOptions>,
  'leftAxis' | 'rightAxis'
> & {
  leftAxis?: Partial<TimeSeriesAxisOptions>;
  rightAxis?: Partial<TimeSeriesAxisOptions>;
};

export interface TimeSeriesRange {
  /**
   * Values use the same timestamp unit as the input timestamps/xDomain.
   * This keeps backend microsecond contracts lossless at integration points.
   */
  from: number;
  to: number;
}

export interface TimeSeriesSignalContext {
  timestamp?: number;
  timeRange?: TimeSeriesRange;
  labels: Record<string, string>;
  serviceName?: string;
  traceId?: string;
  spanId?: string;
}

export interface TimeSeriesStats {
  last: number | null;
  min: number | null;
  max: number | null;
  mean: number | null;
  sum: number | null;
  count: number;
}

export const DEFAULT_TIME_SERIES_OPTIONS: TimeSeriesChartOptions = {
  drawStyle: 'line',
  interpolation: 'linear',
  lineWidth: 1.5,
  fillOpacity: 0,
  showPoints: 'auto',
  stackMode: 'none',
  connectNulls: false,
  tooltipMode: 'all',
  tooltipSort: 'desc',
  tooltipHideZeros: false,
  tooltipMaxItems: 24,
  legendMode: 'table',
  legendPlacement: 'bottom',
  legendStats: ['last', 'min', 'max', 'mean'],
  showXAxis: true,
  showYAxis: true,
  compactAxes: false,
  leftAxis: {
    scale: 'linear',
    showGrid: true,
  },
  thresholds: [],
  bands: [],
  annotations: [],
};
