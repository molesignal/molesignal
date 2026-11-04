/**
 * The figures the RUM pages tell, worked out again from the rows of the mock organization, so that a
 * spec says what a page must say without copying what it says. Every function is the plain statement of
 * what the page does: the 75th percentile by nearest rank, as the mock backend takes it, and the words
 * of a time, a size, a share and a count the way the pages write them.
 */
import type { RumRow } from '../../src/api/rum';

/** The 75th percentile by nearest rank. */
export function p75(values: readonly number[]): number {
  const sorted = [...values].sort((a, b) => a - b);

  return sorted[Math.max(1, Math.ceil(sorted.length * 0.75)) - 1] as number;
}

/** A time in milliseconds as the pages write it: in the unit it reads best in. */
export const duration = (ms: number): string => (ms < 1_000 ? `${Math.round(ms)} ms` : ms < 60_000 ? `${(ms / 1_000).toFixed(1)} s` : `${(ms / 60_000).toFixed(1)} min`);

/** A size in bytes as the pages write it. */
export function size(bytes: number): string {
  if (bytes < 1_024) return `${Math.round(bytes)} B`;
  if (bytes < 1_024 * 1_024) return `${(bytes / 1_024).toFixed(1)} KB`;

  return `${(bytes / (1_024 * 1_024)).toFixed(1)} MB`;
}

/** A share of failures with the decimal it needs: 2.1%, and `<0.1%` for what is there but small. */
export function rate(fraction: number): string {
  if (fraction <= 0) return '0%';
  if (fraction < 0.001) return '<0.1%';

  return `${Number((fraction * 100).toFixed(1))}%`;
}

/** A share in whole percent: `38%`, and `<1%` for what is there but small. */
export function percent(fraction: number): string {
  if (fraction <= 0) return '0%';
  if (fraction < 0.01) return '<1%';

  return `${Math.round(fraction * 100)}%`;
}

export const ms = (row: RumRow, field: string): number | null => {
  const value = row[field];

  return typeof value === 'number' ? value / 1_000_000 : null;
};

/** The rows grouped by the values of their columns, the commonest group first, ties by the values. */
export function grouped(rows: readonly RumRow[], fields: readonly string[]): Array<{ values: Array<string | null>; rows: RumRow[] }> {
  const groups = new Map<string, { values: Array<string | null>; rows: RumRow[] }>();

  for (const row of rows) {
    const values = fields.map((field) => (row[field] === undefined || row[field] === null || row[field] === '' ? null : String(row[field])));
    const key = JSON.stringify(values);
    const entry = groups.get(key) ?? { values, rows: [] };

    entry.rows.push(row);
    groups.set(key, entry);
  }

  return [...groups.values()].sort((a, b) => b.rows.length - a.rows.length || String(a.values[0]).localeCompare(String(b.values[0])) || String(a.values[1]).localeCompare(String(b.values[1])));
}

/** A request that ended badly: an error status, or no answer at all. */
export const failed = (row: RumRow): boolean => typeof row['resource.status_code'] === 'number' && ((row['resource.status_code'] as number) >= 400 || row['resource.status_code'] === 0);

const AXIS_STEPS = [50, 100, 200, 250, 500, 1_000, 2_000, 2_500, 5_000, 10_000, 15_000, 30_000, 60_000];

/** Where the time axis of the waterfall of a view ends, in milliseconds: after the last moment the view says, in whole steps of at most five. */
export function axisEnd(view: RumRow): number {
  const moments = ['view.first_byte', 'view.performance.fcp.timestamp', 'view.dom_content_loaded', 'view.performance.lcp.timestamp', 'view.load_event', 'view.loading_time']
    .map((field) => ms(view, field))
    .filter((value): value is number => value !== null && value >= 0);
  const target = Math.max(Math.max(...moments) * 1.05, 100);
  const step = AXIS_STEPS.find((candidate) => target / candidate <= 5) ?? 120_000;

  return Math.ceil(target / step) * step;
}

/** How long after its view began a row happened, in milliseconds. */
export const after = (row: RumRow, view: RumRow): number => Math.max(0, (row.timestamp - view.timestamp) / 1_000);
