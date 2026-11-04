import { rowsToSeries } from '@/lib/metricsSeries';
import type { QueryResult } from '@/types/query';
import type { TimeSeriesSeries } from '@/viz/timeseries/types';

import type {
  DataField,
  DataFrame,
  FieldType,
  PanelDataSourceType,
} from './schema';

const TIME_FIELD_NAMES = new Set([
  '_timestamp',
  'timestamp',
  'time',
  'ts',
  '__time__',
]);

export function queryResultToDataFrames(
  result: QueryResult,
  refId: string,
  sourceType: PanelDataSourceType,
  legend?: string,
  format?: string,
): DataFrame[] {
  if (sourceType === 'metrics') {
    const series = rowsToSeries(result);
    if (series.length > 0) {
      return series.map((item, index) => ({
        refId,
        name: seriesName(
          item.valueColumn,
          item.labels,
          index,
          legend,
        ),
        length: item.values.length,
        fields: [
          {
            id: `${refId}-${index}-time`,
            name: 'time',
            type: 'time',
            values:
              item.timestamps.length > 0
                ? item.timestamps
                : item.values.map((_, valueIndex) => valueIndex),
          },
          {
            id: `${refId}-${index}-${item.valueColumn}`,
            name: item.valueColumn,
            type: 'number',
            values: item.values.map((value) =>
              Number.isFinite(value) ? value : null,
            ),
            labels: item.labels,
          },
        ],
        meta: {
          sourceType,
          preferredVisualization: 'time_series',
          queryDurationMs: result.tookMs,
          scannedRows: result.scannedRows,
        },
      }));
    }
  }

  const frame = rowsToDataFrame(result.columns, result.rows, {
    refId,
    sourceType,
    queryDurationMs: result.tookMs,
    scannedRows: result.scannedRows,
  });

  return format === 'time_series' ? longToSeriesFrames(frame) : [frame];
}

/**
 * A result formatted as a time series that is long, a row for each time and
 * set of labels, as one frame for each set: its text columns are the labels of
 * its numbers, so a chart draws a line for each, named by them. A result with
 * no text column is one frame, as it was.
 */
export function longToSeriesFrames(frame: DataFrame): DataFrame[] {
  const time = frame.fields.find((field) => field.type === 'time');
  const labels = frame.fields.filter(
    (field) => field.type === 'string' || field.type === 'boolean',
  );
  const numbers = frame.fields.filter((field) => field.type === 'number');
  if (!time || labels.length === 0 || numbers.length === 0) return [frame];

  const groups = new Map<string, { labels: Record<string, string>; rows: number[] }>();
  for (let row = 0; row < frame.length; row += 1) {
    const set = Object.fromEntries(
      labels.flatMap((field) => {
        const value = field.values[row];
        return value === null || value === undefined ? [] : [[field.name, String(value)]];
      }),
    );
    const key = JSON.stringify(labels.map((field) => set[field.name] ?? null));
    const group = groups.get(key) ?? { labels: set, rows: [] };
    group.rows.push(row);
    groups.set(key, group);
  }

  return [...groups.values()].map((group, index) => {
    const pick = (values: readonly unknown[]) => group.rows.map((row) => values[row]);
    const named = Object.values(group.labels).join(' · ');
    return {
      ...frame,
      // One number is named by its labels alone, a row without any as a value that is missing;
      // several numbers keep their own names beside the labels.
      name: numbers.length === 1 ? named || '—' : frame.name,
      length: group.rows.length,
      fields: [
        { ...time, id: `${time.id}-${index}`, values: pick(time.values) },
        ...numbers.map((field) => ({
          ...field,
          id: `${field.id}-${index}`,
          values: pick(field.values),
          labels: group.labels,
        })),
      ],
    };
  });
}

export function rowsToDataFrame(
  columns: readonly string[],
  rows: readonly unknown[][],
  options: {
    refId: string;
    name?: string | undefined;
    sourceType?: string | undefined;
    queryDurationMs?: number | undefined;
    scannedRows?: number | undefined;
  },
): DataFrame {
  const fields: DataField[] = columns.map((name, index) => {
    const values = rows.map((row) => row[index]);
    return {
      id: `${options.refId}-${index}-${name}`,
      name,
      type: inferFieldType(name, values),
      values,
    };
  });
  return {
    refId: options.refId,
    name: options.name,
    length: rows.length,
    fields,
    meta: {
      sourceType: options.sourceType,
      queryDurationMs: options.queryDurationMs,
      scannedRows: options.scannedRows,
    },
  };
}

export function dataFrameToRows(frame: DataFrame): unknown[][] {
  return Array.from({ length: frame.length }, (_, rowIndex) =>
    frame.fields.map((field) => field.values[rowIndex]),
  );
}

export function dataFrameToObjects(
  frame: DataFrame,
): Array<Record<string, unknown>> {
  return dataFrameToRows(frame).map((row) =>
    Object.fromEntries(
      frame.fields.map((field, index) => [field.name, row[index]]),
    ),
  );
}

export function cloneDataFrame(frame: DataFrame): DataFrame {
  return {
    ...frame,
    fields: frame.fields.map((field) => ({
      ...field,
      values: [...field.values],
      labels: field.labels ? { ...field.labels } : undefined,
      config: field.config ? { ...field.config } : undefined,
      meta: field.meta ? { ...field.meta } : undefined,
    })),
    meta: frame.meta ? { ...frame.meta } : undefined,
  };
}

export function framesToTimeSeries(
  frames: readonly DataFrame[],
): TimeSeriesSeries[] {
  const out: TimeSeriesSeries[] = [];
  for (const frame of frames) {
    const time = frame.fields.find((field) => field.type === 'time');
    const timestamps = time?.values.map(toTimestamp);
    for (const field of frame.fields) {
      if (field.type !== 'number') continue;
      const labels = field.labels ?? {};
      const labelSuffix = Object.entries(labels)
        .map(([key, value]) => `${key}=${value}`)
        .join(' · ');
      const displayName =
        field.config?.displayName ??
        frame.name ??
        (labelSuffix ? `${field.name} · ${labelSuffix}` : field.name);
      out.push({
        id: field.id,
        name: displayName,
        data: field.values.map((value) =>
          typeof value === 'number' && Number.isFinite(value) ? value : null,
        ),
        labels,
        ...(timestamps ? { timestamps } : {}),
        ...(field.config?.unit ? { unit: field.config.unit } : {}),
        ...(field.config?.color?.mode === 'fixed' &&
        field.config.color.value
          ? { color: field.config.color.value }
          : {}),
      });
    }
  }
  return out;
}

export function inferFieldType(
  name: string,
  values: readonly unknown[],
): FieldType {
  if (TIME_FIELD_NAMES.has(name.toLowerCase())) return 'time';
  const sample = values.find(
    (value) => value !== null && value !== undefined,
  );
  if (typeof sample === 'number') return 'number';
  if (typeof sample === 'boolean') return 'boolean';
  if (typeof sample === 'object') return 'json';
  return 'string';
}

export function normalizeFrameLength(frame: DataFrame): DataFrame {
  const length = Math.max(
    0,
    ...frame.fields.map((field) => field.values.length),
  );
  return {
    ...frame,
    length,
    fields: frame.fields.map((field) => ({
      ...field,
      values: Array.from(
        { length },
        (_, index) => field.values[index] ?? null,
      ),
    })),
  };
}

function seriesName(
  valueColumn: string,
  labels: Record<string, string>,
  index: number,
  legend?: string,
): string {
  if (legend?.trim() && legend !== '__auto') {
    return renderLegendFormat(legend, labels);
  }
  const suffix = Object.values(labels).filter(Boolean).join(' · ');
  return suffix || valueColumn || `Series ${index + 1}`;
}

/**
 * Replaces Grafana-style `{{label}}` placeholders with series label values.
 * Grafana falls back to the label key when a value is absent or empty.
 */
export function renderLegendFormat(
  pattern: string,
  labels: Readonly<Record<string, string>>,
): string {
  return pattern.replace(
    /\{\{\s*(.+?)\s*\}\}/g,
    (_match, label: string) => labels[label] || label,
  );
}

function toTimestamp(value: unknown, index: number): number {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string') {
    const numeric = Number(value);
    if (Number.isFinite(numeric)) return numeric;
    const date = Date.parse(value);
    if (Number.isFinite(date)) return date;
  }
  return index;
}
