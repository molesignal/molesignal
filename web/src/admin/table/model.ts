/** The plain value behind a cell: what it sorts by and what an export writes. */
export type TableValue = string | number | boolean | null | undefined;

export type SortDirection = 'asc' | 'desc';

export interface TableSort {
  key: string;
  direction: SortDirection;
}

/** A head pressed again goes the other way, a third time back to the list's own order. */
export function nextSort(current: TableSort | null, key: string): TableSort | null {
  if (current?.key !== key) return { key, direction: 'asc' };
  return current.direction === 'asc' ? { key, direction: 'desc' } : null;
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

function isAbsent(value: TableValue): boolean {
  return value === null || value === undefined || value === '';
}

function compareValues(a: TableValue, b: TableValue): number {
  if (typeof a === 'number' && typeof b === 'number') return a - b;
  return collator.compare(String(a), String(b));
}

/** Rows in the order of one column. What has no value goes last either way; ties keep the list's order. */
export function sortRows<T>(
  rows: readonly T[],
  value: (row: T) => TableValue,
  direction: SortDirection,
): T[] {
  const factor = direction === 'asc' ? 1 : -1;

  return rows
    .map((row, index) => ({ row, index, value: value(row) }))
    .sort((a, b) => {
      const absentA = isAbsent(a.value);
      const absentB = isAbsent(b.value);
      if (absentA || absentB) return Number(absentA) - Number(absentB) || a.index - b.index;

      return factor * compareValues(a.value, b.value) || a.index - b.index;
    })
    .map((entry) => entry.row);
}

/** A text that opens with one of these is run as a formula by a spreadsheet. */
const FORMULA_START = /^[=+\-@\t\r]/;

function csvCell(value: TableValue): string {
  if (value === null || value === undefined) return '';
  if (typeof value !== 'string') return String(value);

  const text = FORMULA_START.test(value) ? `'${value}` : value;
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

export interface CsvColumn<T> {
  header: string;
  value: (row: T) => TableValue;
}

/** Comma separated, with the mark that tells a spreadsheet the file is UTF-8, so Chinese opens as Chinese. */
export function rowsToCsv<T>(columns: readonly CsvColumn<T>[], rows: readonly T[]): string {
  const lines = [
    columns.map((column) => csvCell(column.header)),
    ...rows.map((row) => columns.map((column) => csvCell(column.value(row)))),
  ];
  return `\uFEFF${lines.map((cells) => cells.join(',')).join('\r\n')}\r\n`;
}

export function downloadCsv(filename: string, csv: string): void {
  const url = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }));
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

export interface BulkOutcome {
  done: number;
  failed: number;
  firstError: unknown;
}

/** One action over many rows where the backend has no bulk call: all are tried, and what failed is counted. */
export async function runOverRows<T>(rows: readonly T[], act: (row: T) => Promise<unknown>): Promise<BulkOutcome> {
  const results = await Promise.allSettled(rows.map((row) => act(row)));
  const rejected = results.filter((result): result is PromiseRejectedResult => result.status === 'rejected');

  return {
    done: results.length - rejected.length,
    failed: rejected.length,
    firstError: rejected[0]?.reason,
  };
}
