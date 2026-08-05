import { formatTimeSeriesTimestamp } from '@/viz/timeseries/formatters';

import { formatFieldValue } from '../../fieldConfig';
import type { DataField, DataFrame } from '../../schema';
import { visualizationColors } from '../shared/colors';
import { zeroInclusiveRange, type ValueRange } from '../shared/range';
import {
  numericDisplayValues,
  type Calculation,
} from '../shared/reduction';
import { normalizeTimestamp } from '../shared/time';

export const MAX_BAR_CHART_CATEGORIES = 120;

export interface BarChartSeries {
  id: string;
  name: string;
  color: string;
}

export interface BarChartPoint {
  value: number;
  text: string;
  color: string;
}

export interface BarChartCategory {
  id: string;
  label: string;
  values: Record<string, BarChartPoint | undefined>;
}

export interface BarChartModel {
  categories: BarChartCategory[];
  series: BarChartSeries[];
  range: ValueRange;
  truncated: boolean;
}

interface SeriesDraft {
  id: string;
  name: string;
  fixedColor?: string | undefined;
}

export function prepareBarChart(
  frames: readonly DataFrame[],
  calculation: Calculation = 'last',
  categoryLimit = MAX_BAR_CHART_CATEGORIES,
): BarChartModel | null {
  const categoryFrames = frames.filter((frame) => categoryField(frame));
  if (categoryFrames.length === 0) {
    return prepareReducedBarChart(frames, calculation, categoryLimit);
  }

  const categories = new Map<string, BarChartCategory>();
  const seriesDrafts = new Map<string, SeriesDraft>();
  for (const frame of categoryFrames) {
    const category = categoryField(frame)!;
    const numericFields = frame.fields.filter((field) => field.type === 'number');
    for (const field of numericFields) {
      const id = seriesId(frame, field);
      seriesDrafts.set(id, {
        id,
        name: field.config?.displayName ?? field.name,
        fixedColor:
          field.config?.color?.mode === 'fixed'
            ? field.config.color.value
            : undefined,
      });
    }

    const rowCount = Math.min(frame.length, category.values.length);
    for (let index = 0; index < rowCount; index += 1) {
      const label = categoryLabel(category, category.values[index]);
      const entry = categories.get(label) ?? {
        id: label,
        label,
        values: {},
      };
      for (const field of numericFields) {
        const raw = field.values[index];
        if (typeof raw !== 'number' || !Number.isFinite(raw)) continue;
        const display = formatFieldValue(raw, field.config);
        entry.values[seriesId(frame, field)] = {
          value: raw,
          text: display.text,
          color:
            display.color ??
            (field.config?.color?.mode === 'fixed'
              ? field.config.color.value ?? ''
              : ''),
        };
      }
      categories.set(label, entry);
    }
  }

  const drafts = [...seriesDrafts.values()];
  const colors = visualizationColors(drafts.map((series) => series.id));
  const series = drafts.map((draft, index) => ({
    id: draft.id,
    name: draft.name,
    color: draft.fixedColor ?? colors[index]!,
  }));
  const colorBySeries = new Map(series.map((item) => [item.id, item.color]));
  for (const category of categories.values()) {
    for (const [id, point] of Object.entries(category.values)) {
      if (point && !point.color) point.color = colorBySeries.get(id) ?? 'var(--accent)';
    }
  }
  return finalizeModel([...categories.values()], series, categoryLimit);
}

function prepareReducedBarChart(
  frames: readonly DataFrame[],
  calculation: Calculation,
  categoryLimit: number,
): BarChartModel | null {
  const values = numericDisplayValues(frames, calculation);
  if (values.length === 0) return null;
  const colors = visualizationColors(values.map((item) => item.key));
  const categories = values.map((item, index): BarChartCategory => {
    const display = formatFieldValue(item.value, item.field.config);
    return {
      id: item.key,
      label: item.field.config?.displayName ?? item.field.name,
      values: {
        value: {
          value: item.value,
          text: display.text,
          color:
            display.color ??
            item.field.config?.color?.value ??
            colors[index]!,
        },
      },
    };
  });
  return finalizeModel(
    categories,
    [{ id: 'value', name: 'Value', color: 'var(--accent)' }],
    categoryLimit,
  );
}

function finalizeModel(
  categories: BarChartCategory[],
  series: BarChartSeries[],
  categoryLimit: number,
): BarChartModel | null {
  if (categories.length === 0 || series.length === 0) return null;
  const safeLimit = Math.max(1, Math.floor(categoryLimit));
  const truncated = categories.length > safeLimit;
  const visible = truncated ? categories.slice(-safeLimit) : categories;
  const values = visible.flatMap((category) =>
    Object.values(category.values).flatMap((point) =>
      point ? [point.value] : [],
    ),
  );
  if (values.length === 0) return null;
  return { categories: visible, series, range: zeroInclusiveRange(values), truncated };
}

function categoryField(frame: DataFrame): DataField | undefined {
  return frame.fields.find((field) =>
    ['string', 'enum', 'boolean', 'time'].includes(field.type),
  );
}

function categoryLabel(field: DataField, value: unknown): string {
  if (field.type === 'time') {
    const timestamp = normalizeTimestamp(value);
    if (timestamp !== null) return formatTimeSeriesTimestamp(timestamp);
  }
  return formatFieldValue(value, field.config).text;
}

function seriesId(frame: DataFrame, field: DataField): string {
  return `${frame.refId}:${field.id}`;
}
