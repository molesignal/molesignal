import type { QueryResult } from '@/types/query';

/**
 * Multi-series adapter for Metrics page.
 *
 * Backend `/query` returns flat `{ columns, rows }`; this util reshapes it into
 * `MetricSeries[]` keyed by label dimensions so the chart can render one line
 * per label combination and the legend can attach SignalReference HoverCards
 * to each `service` / `host` cell.
 *
 * Column classification (purely heuristic on column NAMES + first row value):
 *  - `time`  — `_timestamp` / `ts` / `time` / `timestamp` (case-insensitive)
 *  - `value` — any numeric column that isn't a time column
 *  - `label` — everything else (string, boolean, or non-numeric)
 *
 * If multiple numeric columns are present, the first non-time numeric column
 * is treated as the value axis; the rest are ignored (the legend surfaces
 * the column name so users know which one is plotted).
 */

export type MetricColumnKind = 'label' | 'value' | 'time';

export interface MetricColumnInfo {
  name: string;
  kind: MetricColumnKind;
}

export interface MetricSeries {
  /** Stable label set defining this series — e.g. `{ service: "foo", host: "node-3" }` */
  labels: Record<string, string>;
  /** Numeric column extracted as the value axis, ordered by row position. */
  values: number[];
  /** Microsecond timestamps aligned 1:1 with `values`. Empty when no time column. */
  timestamps: number[];
  /** Column name that supplied `values` — surfaced in legend for clarity. */
  valueColumn: string;
}

const TIME_COLUMN_NAMES = ['_timestamp', 'timestamp', 'ts', 'time', '__time__'];

function isTimeColumn(name: string): boolean {
  const lower = name.toLowerCase();
  return TIME_COLUMN_NAMES.some((c) => c === lower);
}

function isNumber(v: unknown): v is number {
  return typeof v === 'number' && Number.isFinite(v);
}

/** Classify each column. Falls back to `label` for anything not numeric. */
export function classifyColumns(result: QueryResult): MetricColumnInfo[] {
  const firstRow = result.rows[0];
  return result.columns.map((name, i) => {
    if (isTimeColumn(name)) return { name, kind: 'time' };
    const sample = firstRow?.[i];
    if (isNumber(sample)) return { name, kind: 'value' };
    return { name, kind: 'label' };
  });
}

/**
 * Reshape a flat `QueryResult` into one `MetricSeries` per label-tuple.
 *
 * Empty result → empty array. Single-series result (no label columns) → one
 * `MetricSeries` with `labels: {}`. Multi-series (e.g. `sum by (service)`) →
 * one entry per unique label tuple, ordered by first appearance.
 */
export function rowsToSeries(result: QueryResult): MetricSeries[] {
  if (result.rows.length === 0) return [];
  const info = classifyColumns(result);
  const valueIdx = info.findIndex((c) => c.kind === 'value');
  if (valueIdx < 0) return [];
  const timeIdx = info.findIndex((c) => c.kind === 'time');
  const labelIdxs = info
    .map((c, i) => (c.kind === 'label' ? i : -1))
    .filter((i) => i >= 0);

  const valueColumn = info[valueIdx]!.name;

  // Keyed by JSON of labels object so different orderings of the same labels
  // map to the same key (label sets are small; this is fine).
  const seriesByKey = new Map<string, MetricSeries>();
  // Preserve first-seen order so chart colors stay stable across renders.
  const orderedKeys: string[] = [];

  for (const row of result.rows) {
    const labels: Record<string, string> = {};
    for (const idx of labelIdxs) {
      const raw = row[idx];
      if (raw === null || raw === undefined) continue;
      labels[result.columns[idx]!] = String(raw);
    }
    const key = JSON.stringify(labels);
    let series = seriesByKey.get(key);
    if (!series) {
      series = { labels, values: [], timestamps: [], valueColumn };
      seriesByKey.set(key, series);
      orderedKeys.push(key);
    }
    const value = row[valueIdx];
    series.values.push(isNumber(value) ? value : Number.NaN);
    if (timeIdx >= 0) {
      const ts = row[timeIdx];
      series.timestamps.push(isNumber(ts) ? ts : 0);
    }
  }

  return orderedKeys.map((k) => seriesByKey.get(k)!);
}

/** Pick the "top" series for sparkline / single-series fallback views. */
export function topSeries(
  series: MetricSeries[],
  by: 'last' | 'max' = 'last',
): MetricSeries | null {
  if (series.length === 0) return null;
  let best = series[0]!;
  let bestScore = scoreSeries(best, by);
  for (let i = 1; i < series.length; i++) {
    const s = series[i]!;
    const score = scoreSeries(s, by);
    if (score > bestScore) {
      best = s;
      bestScore = score;
    }
  }
  return best;
}

function scoreSeries(s: MetricSeries, by: 'last' | 'max'): number {
  if (s.values.length === 0) return -Infinity;
  if (by === 'last') {
    const v = s.values[s.values.length - 1];
    return isNumber(v) ? v : -Infinity;
  }
  let max = -Infinity;
  for (const v of s.values) if (isNumber(v) && v > max) max = v;
  return max;
}

/** Per-series stats keyed by the same JSON label key used by `rowsToSeries`. */
export interface SeriesStats {
  min: number;
  p50: number;
  avg: number;
  p95: number;
  p99: number;
  max: number;
  last: number;
}

export interface MetricSeriesQuality {
  dataPoints: number;
  missingPoints: number;
  missingRatio: number;
  negativePoints: number;
  timestampAnomalies: number;
  estimatedStepSeconds: number | null;
}

export function seriesStats(s: MetricSeries): SeriesStats | null {
  const finite = s.values.filter(isNumber);
  if (finite.length === 0) return null;
  const sorted = [...finite].sort((a, b) => a - b);
  const at = (p: number) => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))]!;
  const avg = finite.reduce((a, b) => a + b, 0) / finite.length;
  return {
    min: sorted[0]!,
    p50: at(0.5),
    avg,
    p95: at(0.95),
    p99: at(0.99),
    max: sorted[sorted.length - 1]!,
    last: finite[finite.length - 1]!,
  };
}

/**
 * Quality signals shown next to a metrics query. Missing values remain
 * distinct from real zeroes; duplicate/out-of-order timestamps are reported
 * instead of silently presented as an ordinary line.
 */
export function analyzeMetricSeries(
  series: ReadonlyArray<MetricSeries>,
): MetricSeriesQuality {
  let dataPoints = 0;
  let missingPoints = 0;
  let negativePoints = 0;
  let timestampAnomalies = 0;
  const positiveDeltas: number[] = [];

  for (const item of series) {
    for (const value of item.values) {
      if (!isNumber(value)) {
        missingPoints += 1;
        continue;
      }
      dataPoints += 1;
      if (value < 0) negativePoints += 1;
    }
    for (let index = 1; index < item.timestamps.length; index += 1) {
      const previous = item.timestamps[index - 1]!;
      const current = item.timestamps[index]!;
      if (!Number.isFinite(previous) || !Number.isFinite(current)) continue;
      const delta = current - previous;
      if (delta <= 0) timestampAnomalies += 1;
      else positiveDeltas.push(delta);
    }
  }

  positiveDeltas.sort((left, right) => left - right);
  const medianDelta =
    positiveDeltas.length === 0
      ? null
      : positiveDeltas[Math.floor(positiveDeltas.length / 2)]!;
  const totalSlots = dataPoints + missingPoints;
  return {
    dataPoints,
    missingPoints,
    missingRatio: totalSlots === 0 ? 0 : missingPoints / totalSlots,
    negativePoints,
    timestampAnomalies,
    estimatedStepSeconds:
      medianDelta === null ? null : normalizeTimestampDeltaToSeconds(medianDelta),
  };
}

/** Short, user-facing series alias; full labels remain available on hover. */
export function compactMetricSeriesName(series: MetricSeries): string {
  const labelPriority = [
    'service',
    'service.name',
    'method',
    'http.method',
    'route',
    'http.route',
    'status',
    'status_code',
    'http.status_code',
  ];
  const values: string[] = [];
  for (const key of labelPriority) {
    const value = series.labels[key];
    if (value && !values.includes(value)) values.push(value);
  }
  if (values.length === 0) {
    values.push(...Object.values(series.labels).filter(Boolean).slice(0, 4));
  }
  return values.length > 0 ? values.join(' · ') : series.valueColumn;
}

export function fullMetricSeriesName(series: MetricSeries): string {
  const entries = Object.entries(series.labels);
  if (entries.length === 0) return series.valueColumn;
  return entries.map(([key, value]) => `${key}="${value}"`).join(' · ');
}

/** Grafana Explore-style identity: `metric{label="value", ...}`. */
export function grafanaMetricSeriesName(
  series: MetricSeries,
  metricName?: string,
): string {
  const name = metricName?.trim() || series.valueColumn;
  const entries = Object.entries(series.labels).sort(([left], [right]) =>
    left.localeCompare(right),
  );
  if (entries.length === 0) return name;
  const labels = entries
    .map(([key, value]) => `${key}=${JSON.stringify(value)}`)
    .join(', ');
  return `${name}{${labels}}`;
}

function normalizeTimestampDeltaToSeconds(delta: number): number {
  // QueryResult timestamps are microseconds by contract.
  return delta / 1_000_000;
}

/**
 * Aggregate stats across all series: each percentile is the **max** across
 * per-series percentiles. Matches the SRE intuition "the worst series sets
 * the alert" rather than the legacy "flatten all rows then compute".
 */
export function aggregateStats(series: MetricSeries[]): SeriesStats | null {
  const perSeries = series.map(seriesStats).filter((s): s is SeriesStats => s !== null);
  if (perSeries.length === 0) return null;
  return {
    min: Math.min(...perSeries.map((s) => s.min)),
    p50: Math.max(...perSeries.map((s) => s.p50)),
    avg: Math.max(...perSeries.map((s) => s.avg)),
    p95: Math.max(...perSeries.map((s) => s.p95)),
    p99: Math.max(...perSeries.map((s) => s.p99)),
    max: Math.max(...perSeries.map((s) => s.max)),
    last: Math.max(...perSeries.map((s) => s.last)),
  };
}
