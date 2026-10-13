import { displayLogValue } from '../viewModel';

/** A field of a record as the field list shows it: a leaf under its dotted path. */
export interface LogFieldRow {
  path: string;
  value: unknown;
}

const TIMESTAMP_FIELDS = new Set(['_timestamp', 'timestamp', 'time']);

function isPresent(value: unknown): boolean {
  return value !== null && value !== undefined && value !== '';
}

/** Every leaf of a record under its dotted path, in the record's order. A list is one value. */
export function flattenRecord(record: Record<string, unknown>, prefix = ''): LogFieldRow[] {
  return Object.entries(record).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;

    return value && typeof value === 'object' && !Array.isArray(value)
      ? flattenRecord(value as Record<string, unknown>, path)
      : [{ path, value }];
  });
}

/**
 * The rows of the field list: every field but the ones the head of the drawer
 * already says in full, which are the moment and the message.
 */
export function logFieldRows(record: Record<string, unknown>, statementField: string | null): LogFieldRow[] {
  return flattenRecord(record).filter(
    (row) => !TIMESTAMP_FIELDS.has(row.path) && row.path !== statementField,
  );
}

/** The rows whose name or value holds `needle`, whatever its case. */
export function matchFieldRows(rows: readonly LogFieldRow[], needle: string): LogFieldRow[] {
  const sought = needle.trim().toLowerCase();
  if (!sought) return [...rows];

  return rows.filter(
    (row) => row.path.toLowerCase().includes(sought) || fieldValueText(row.value).toLowerCase().includes(sought),
  );
}

/** A value as the field list writes it. Nothing is written as the word a query would use for it. */
export function fieldValueText(value: unknown): string {
  if (value === null) return 'null';
  if (value === undefined) return '';

  return displayLogValue(value);
}

/** Whether a value can be asked for in a filter. */
export function isFilterableValue(value: unknown): boolean {
  return isPresent(value);
}

export function logRecordJson(record: Record<string, unknown>): string {
  return JSON.stringify(record, null, 2);
}

/** A file name for one log, from its moment. */
export function logFileName(moment: string, position: number): string {
  const safe = moment.replace(/[^0-9A-Za-z_-]+/g, '-').replace(/^-+|-+$/g, '') || String(position);

  return `molesignal-log-${safe}.json`;
}
