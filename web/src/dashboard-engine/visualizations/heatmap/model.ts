import { formatTimeSeriesTimestamp } from '@/viz/timeseries/formatters';

import type { DataFrame } from '../../schema';
import { normalizeTimestamp } from '../shared/time';

export const MAX_HEATMAP_COLUMNS = 120;

export interface HeatmapRow {
  id: string;
  name: string;
  values: Array<number | null>;
}

export interface HeatmapModel {
  rows: HeatmapRow[];
  columns: number;
  totalSamples: number;
  windowSize: number;
  firstColumnLabel: string;
  lastColumnLabel: string;
  min: number;
  max: number;
  constant: boolean;
}

export function prepareHeatmap(
  frames: readonly DataFrame[],
  columnLimit = MAX_HEATMAP_COLUMNS,
): HeatmapModel | null {
  const fields = frames.flatMap((frame) =>
    frame.fields
      .filter((field) => field.type === 'number')
      .map((field) => ({ frame, field })),
  );
  const totalSamples = Math.max(0, ...fields.map(({ field }) => field.values.length));
  if (fields.length === 0 || totalSamples === 0) return null;

  const safeLimit = Math.max(1, Math.floor(columnLimit));
  const windowSize = Math.max(1, Math.ceil(totalSamples / safeLimit));
  const rows = fields.map(({ frame, field }): HeatmapRow => ({
    id: `${frame.refId}:${field.id}`,
    name:
      field.config?.displayName ??
      (frame.name ? `${frame.name} · ${field.name}` : field.name),
    values: aggregateFiniteWindows(field.values, totalSamples, windowSize),
  }));
  const finite = rows.flatMap((row) =>
    row.values.filter((value): value is number => value !== null),
  );
  if (finite.length === 0) return null;
  const rawMin = Math.min(...finite);
  const rawMax = Math.max(...finite);
  const constant = rawMin === rawMax;
  const timeValues = frames
    .flatMap((frame) => frame.fields)
    .find((field) => field.type === 'time')?.values;
  const columns = rows[0]?.values.length ?? 0;

  return {
    rows,
    columns,
    totalSamples,
    windowSize,
    firstColumnLabel: columnLabel(timeValues?.[0], 1),
    lastColumnLabel: columnLabel(timeValues?.[totalSamples - 1], totalSamples),
    min: rawMin,
    max: rawMax,
    constant,
  };
}

export function aggregateFiniteWindows(
  values: readonly unknown[],
  totalSamples: number,
  windowSize: number,
): Array<number | null> {
  const output: Array<number | null> = [];
  for (let start = 0; start < totalSamples; start += windowSize) {
    const finite = values
      .slice(start, Math.min(totalSamples, start + windowSize))
      .filter(
        (value): value is number =>
          typeof value === 'number' && Number.isFinite(value),
      );
    output.push(
      finite.length > 0
        ? finite.reduce((sum, value) => sum + value, 0) / finite.length
        : null,
    );
  }
  return output;
}

export function heatmapIntensity(
  value: number | null,
  model: Pick<HeatmapModel, 'min' | 'max' | 'constant'>,
): number {
  if (value === null || !Number.isFinite(value)) return 0;
  if (model.constant || model.max === model.min) return 0.56;
  const ratio = Math.min(1, Math.max(0, (value - model.min) / (model.max - model.min)));
  return 0.12 + ratio * 0.88;
}

function columnLabel(value: unknown, fallback: number): string {
  const timestamp = normalizeTimestamp(value);
  return timestamp === null ? String(fallback) : formatTimeSeriesTimestamp(timestamp);
}
