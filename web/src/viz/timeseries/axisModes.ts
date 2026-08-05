import type uPlot from 'uplot';

import type {
  TimeSeriesAxisOptions,
  TimeSeriesStackMode,
} from './types';

export type AxisMode =
  | 'linear'
  | 'log'
  | 'log2'
  | 'log10'
  | 'symlog'
  | 'percentile';

export interface AxisSpec {
  key: string;
  mode: AxisMode;
  label?: string;
  /** For percentile mode: the breakpoints in [0, 1]. Default [0.5, 0.9, 0.95, 0.99, 0.999]. */
  percentileBreaks?: number[];
}

export function resolveStackedAxisOptions(
  axis: TimeSeriesAxisOptions,
  stackMode: TimeSeriesStackMode,
): TimeSeriesAxisOptions {
  return stackMode === 'percent'
    ? { ...axis, min: 0, max: 100, unit: '%' }
    : axis;
}

/**
 * Build the uPlot scale + axis config for a single y-axis spec. The wrapper
 * applies these via `plot.batch()` when mode toggles, so the uPlot instance
 * is not recreated.
 */
export function buildScale(spec: AxisSpec): NonNullable<uPlot.Options['scales']>[string] {
  switch (spec.mode) {
    case 'log':
    case 'log10':
      return {
        distr: 3, // logarithmic
        log: 10,
        // Drop non-positive points from the visible range.
        range: (_u, _min, max) => [Math.max(_min, Number.MIN_VALUE), max],
      };
    case 'log2':
      return {
        distr: 3,
        log: 2,
        range: (_u, _min, max) => [Math.max(_min, Number.MIN_VALUE), max],
      };
    case 'symlog':
      return {
        distr: 4,
        asinh: 10,
      };
    case 'percentile': {
      const breaks = spec.percentileBreaks ?? [0.5, 0.9, 0.95, 0.99, 0.999];
      return {
        distr: 100, // 100 = custom mapping
        // uPlot's custom-distr API expects `fwd`/`bwd` functions; supply a
        // simple monotone remap that spaces the configured breakpoints evenly.
        forward: (v: number) => percentileForward(v, breaks),
        backward: (v: number) => percentileBackward(v, breaks),
      } as unknown as NonNullable<uPlot.Options['scales']>[string];
    }
    default:
      return { distr: 1 };
  }
}

/**
 * Build a configured chart axis without ever returning a zero-width range.
 * uPlot cannot position a flat series against `[value, value]`; keep the
 * configured baseline and add a small upper/lower span instead.
 */
export function buildTimeSeriesAxisScale(
  axis: TimeSeriesAxisOptions,
): uPlot.Scale {
  const mode: AxisSpec['mode'] =
    axis.scale === 'linear'
      ? 'linear'
      : axis.scale === 'log2'
        ? 'log2'
        : axis.scale === 'log10'
          ? 'log10'
          : 'symlog';
  const base = buildScale({ key: 'y', mode });
  if (
    axis.min === undefined &&
    axis.max === undefined &&
    axis.softMin === undefined &&
    axis.softMax === undefined
  ) {
    return base;
  }
  return {
    ...base,
    range: (_plot, dataMin, dataMax) => {
      const min = axis.min ?? Math.min(dataMin, axis.softMin ?? dataMin);
      const max = axis.max ?? Math.max(dataMax, axis.softMax ?? dataMax);
      return expandDegenerateRange(
        min,
        max,
        axis.min !== undefined || axis.softMin !== undefined,
        axis.max !== undefined || axis.softMax !== undefined,
      );
    },
  };
}

function expandDegenerateRange(
  min: number,
  max: number,
  preserveMin: boolean,
  preserveMax: boolean,
): [number, number] {
  if (!Number.isFinite(min) || !Number.isFinite(max) || min < max) {
    return [min, max];
  }
  if (min > max) return [min, max];

  const padding = min === 0
    ? 1
    : Math.max(Math.abs(min) * 0.1, Number.EPSILON);
  if (preserveMin) return [min, min + padding];
  if (preserveMax) return [max - padding, max];
  return [min - padding, max + padding];
}

function percentileForward(v: number, breaks: number[]): number {
  // breaks are non-uniform percentile cutoffs on [0,1]; we want them at
  // equal pixel distances. Build a piecewise-linear map.
  const segments = breaks.length + 1;
  if (v <= 0) return 0;
  if (v >= 1) return 1;
  for (let i = 0; i < breaks.length; i++) {
    const lo = i === 0 ? 0 : breaks[i - 1]!;
    const hi = breaks[i]!;
    if (v < hi) {
      const local = (v - lo) / (hi - lo);
      return (i + local) / segments;
    }
  }
  const lo = breaks[breaks.length - 1]!;
  const local = (v - lo) / (1 - lo);
  return (segments - 1 + local) / segments;
}

function percentileBackward(v: number, breaks: number[]): number {
  const segments = breaks.length + 1;
  const seg = Math.floor(v * segments);
  const local = v * segments - seg;
  const lo = seg === 0 ? 0 : breaks[seg - 1] ?? 0;
  const hi = breaks[seg] ?? 1;
  return lo + local * (hi - lo);
}
