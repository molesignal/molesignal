import type uPlot from 'uplot';

import { lttb } from './lttb';
import type {
  TimeSeriesSeries,
  TimeSeriesStackMode,
  TimeSeriesStats,
} from './types';

export interface PreparedTimeSeries {
  data: uPlot.AlignedData;
  rawData: uPlot.AlignedData;
  hasTime: boolean;
  inputTimestampScale: number;
  xDomain?: [number, number];
  pointCount: number;
}

interface NormalizedSeries {
  xs: number[];
  ys: Array<number | null>;
}

export function prepareTimeSeriesData(
  series: ReadonlyArray<TimeSeriesSeries>,
  xDomain: readonly [number, number] | undefined,
  stackMode: TimeSeriesStackMode,
): PreparedTimeSeries {
  const inputTimestampScale = detectTimestampScale(series, xDomain);
  const normalizedDomain = xDomain
    ? ([
        xDomain[0] * inputTimestampScale,
        xDomain[1] * inputTimestampScale,
      ] as [number, number])
    : undefined;
  const hasExplicitTime =
    Boolean(xDomain) ||
    series.some(
      (item) =>
        item.timestamps?.length === item.data.length &&
        item.timestamps.some((timestamp) => Number.isFinite(timestamp)),
    );
  const maxLength = Math.max(0, ...series.map((item) => item.data.length));
  const normalized = series.map((item) =>
    normalizeSeries(item, inputTimestampScale, normalizedDomain, hasExplicitTime),
  );

  let rawData: uPlot.AlignedData;
  if (normalized.length === 0 || maxLength === 0) {
    rawData = [[]] as uPlot.AlignedData;
  } else if (shareXAxis(normalized)) {
    rawData = [
      normalized[0]!.xs,
      ...normalized.map((item) => item.ys),
    ] as uPlot.AlignedData;
  } else {
    rawData = alignNormalizedSeries(normalized);
  }

  return {
    data: stackMode === 'none' ? rawData : stackData(rawData, stackMode),
    rawData,
    hasTime: hasExplicitTime,
    inputTimestampScale,
    ...(normalizedDomain ? { xDomain: normalizedDomain } : {}),
    pointCount: rawData[0]?.length ?? 0,
  };
}

export function downsampleAlignedData(
  data: uPlot.AlignedData,
  targetSize: number,
): uPlot.AlignedData {
  const columns = alignedColumns(data);
  const xs = (columns[0] ?? []).map((value) => Number(value));
  if (xs.length <= targetSize || targetSize < 3 || data.length <= 1) return data;

  const ranges = columns.slice(1).map((values) => finiteRange(values));
  const envelope = xs.map((x, index) => {
    let score = 0;
    for (let seriesIndex = 1; seriesIndex < columns.length; seriesIndex += 1) {
      const value = columns[seriesIndex]?.[index];
      if (typeof value !== 'number' || !Number.isFinite(value)) continue;
      const range = ranges[seriesIndex - 1]!;
      const normalized =
        range.max > range.min ? (value - range.min) / (range.max - range.min) : Math.abs(value);
      score = Math.max(score, Math.abs(normalized));
    }
    return [x, score] as [number, number];
  });
  const sampled = lttb(envelope, targetSize);
  const indexByX = new Map<number, number>();
  xs.forEach((x, index) => indexByX.set(x, index));
  const indexes = sampled
    .map(([x]) => indexByX.get(x))
    .filter((index): index is number => index !== undefined);
  return columns.map((column) => indexes.map((index) => column[index] ?? null)) as uPlot.AlignedData;
}

export function selectAlignedDataByX(
  data: uPlot.AlignedData,
  selectedXs: ReadonlyArray<number | null>,
): uPlot.AlignedData {
  const columns = alignedColumns(data);
  const xs = (columns[0] ?? []).map((value) => Number(value));
  const indexByX = new Map<number, number>();
  xs.forEach((x, index) => indexByX.set(x, index));
  const indexes = selectedXs
    .map((x) => (typeof x === 'number' ? indexByX.get(x) : undefined))
    .filter((index): index is number => index !== undefined);
  return columns.map((column) => indexes.map((index) => column[index] ?? null)) as uPlot.AlignedData;
}

export function calculateTimeSeriesStats(values: ReadonlyArray<number | null>): TimeSeriesStats {
  const finite = values.filter(
    (value): value is number => typeof value === 'number' && Number.isFinite(value),
  );
  if (finite.length === 0) {
    return { last: null, min: null, max: null, mean: null, sum: null, count: 0 };
  }
  const sum = finite.reduce((total, value) => total + value, 0);
  return {
    last: finite.at(-1) ?? null,
    min: Math.min(...finite),
    max: Math.max(...finite),
    mean: sum / finite.length,
    sum,
    count: finite.length,
  };
}

export function toInputTimestamp(valueInSeconds: number, inputTimestampScale: number): number {
  return valueInSeconds / inputTimestampScale;
}

function detectTimestampScale(
  series: ReadonlyArray<TimeSeriesSeries>,
  xDomain: readonly [number, number] | undefined,
): number {
  const candidate =
    xDomain?.find((value) => Number.isFinite(value) && value !== 0) ??
    series
      .flatMap((item) => item.timestamps?.slice(0, 2) ?? [])
      .find((value) => Number.isFinite(value) && value !== 0);
  if (candidate === undefined) return 1;
  const absolute = Math.abs(candidate);
  if (absolute >= 1e14) return 1 / 1_000_000;
  if (absolute >= 1e11) return 1 / 1000;
  return 1;
}

function normalizeSeries(
  series: TimeSeriesSeries,
  timestampScale: number,
  domain: readonly [number, number] | undefined,
  hasExplicitTime: boolean,
): NormalizedSeries {
  const values = series.data.map((value) =>
    typeof value === 'number' && Number.isFinite(value) ? value : null,
  );
  const timestampsValid =
    series.timestamps?.length === values.length &&
    series.timestamps.every((timestamp) => Number.isFinite(timestamp));
  let xs: number[];
  if (timestampsValid && series.timestamps) {
    const seriesTimestampScale =
      timestampScaleFor(
        series.timestamps.find((timestamp) => Number.isFinite(timestamp) && timestamp !== 0),
      ) ?? timestampScale;
    xs = series.timestamps.map((timestamp) => timestamp * seriesTimestampScale);
  } else if (domain && values.length > 0) {
    const span = domain[1] - domain[0];
    xs = values.map((_, index) =>
      values.length === 1 ? domain[1] : domain[0] + (span * index) / (values.length - 1),
    );
  } else {
    xs = values.map((_, index) => index);
  }

  if (!hasExplicitTime) return { xs, ys: values };
  const pairs = xs
    .map((x, index) => ({ x, y: values[index] ?? null, index }))
    .filter(({ x }) => Number.isFinite(x))
    .sort((left, right) => left.x - right.x || left.index - right.index);
  const deduped = new Map<number, number | null>();
  for (const pair of pairs) deduped.set(pair.x, pair.y);
  return {
    xs: [...deduped.keys()],
    ys: [...deduped.values()],
  };
}

function timestampScaleFor(candidate: number | undefined): number | undefined {
  if (candidate === undefined) return undefined;
  const absolute = Math.abs(candidate);
  if (absolute >= 1e14) return 1 / 1_000_000;
  if (absolute >= 1e11) return 1 / 1000;
  return 1;
}

function shareXAxis(series: ReadonlyArray<NormalizedSeries>): boolean {
  const first = series[0]?.xs;
  if (!first) return true;
  return series.every(
    (item) =>
      item.xs.length === first.length &&
      item.xs.every((value, index) => value === first[index]),
  );
}

function alignNormalizedSeries(series: ReadonlyArray<NormalizedSeries>): uPlot.AlignedData {
  const xs = [...new Set(series.flatMap((item) => item.xs))].sort((left, right) => left - right);
  const columns: Array<Array<number | null>> = [xs];
  for (const item of series) {
    const valuesByX = new Map(item.xs.map((x, index) => [x, item.ys[index] ?? null]));
    columns.push(xs.map((x) => valuesByX.get(x) ?? null));
  }
  return columns as uPlot.AlignedData;
}

function stackData(data: uPlot.AlignedData, mode: Exclude<TimeSeriesStackMode, 'none'>): uPlot.AlignedData {
  const columns = alignedColumns(data);
  const xs = (columns[0] ?? []).map((value) => Number(value));
  const output: Array<Array<number | null>> = [xs];
  const totals =
    mode === 'percent'
      ? xs.map((_, index) =>
          columns
            .slice(1)
            .reduce((total, column) => {
              const value = column[index];
              return total + (typeof value === 'number' && Number.isFinite(value) ? Math.max(0, value) : 0);
            }, 0),
        )
      : [];
  const accumulated = xs.map(() => 0);

  for (const column of columns.slice(1)) {
    output.push(
      column.map((value, index) => {
        const finite = typeof value === 'number' && Number.isFinite(value) ? value : 0;
        const normalized =
          mode === 'percent' ? (totals[index]! > 0 ? (Math.max(0, finite) / totals[index]!) * 100 : 0) : finite;
        accumulated[index] = accumulated[index]! + normalized;
        return accumulated[index]!;
      }),
    );
  }
  return output as uPlot.AlignedData;
}

function finiteRange(values: ReadonlyArray<number | null>): { min: number; max: number } {
  let min = Number.POSITIVE_INFINITY;
  let max = Number.NEGATIVE_INFINITY;
  for (const value of values) {
    if (typeof value !== 'number' || !Number.isFinite(value)) continue;
    min = Math.min(min, value);
    max = Math.max(max, value);
  }
  return Number.isFinite(min) ? { min, max } : { min: 0, max: 0 };
}

function alignedColumns(data: uPlot.AlignedData): Array<Array<number | null>> {
  const columns = data as unknown as ReadonlyArray<ArrayLike<number | null>>;
  return Array.from(columns, (column) => Array.from(column));
}
