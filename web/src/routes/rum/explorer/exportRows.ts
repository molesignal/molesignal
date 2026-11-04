import { downloadCsv, rowsToCsv } from '@/admin/table/model';
import * as rumApi from '@/api/rum';
import type { RumRow } from '@/api/rum';

import type { ParsedRumStatement } from './fieldQuery';
import type { RumTab } from './model';

/** The most rows a search answers with, and so the most a file holds. */
export const MAX_EXPORTED_ROWS = 5_000;

/** A cell of the file: a number or text as it is, a JSON document as text. */
function cellOf(value: unknown): string | number | null {
  if (value === null || value === undefined) return null;
  if (typeof value === 'number' || typeof value === 'string') return value;

  return JSON.stringify(value);
}

function stamp(): string {
  return new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-');
}

/** The file of rows: when each is dated, in microseconds and readable, then the columns on screen. */
export function rumRowsToCsv(rows: readonly RumRow[], columns: readonly string[]): string {
  return rowsToCsv<RumRow>(
    [
      { header: 'time', value: (row) => new Date(row.timestamp / 1_000).toISOString() },
      ...columns.map((name) => ({ header: name, value: (row: RumRow) => cellOf(row[name]) })),
    ],
    rows,
  );
}

/**
 * Writes the rows the query matches to a CSV file, as far as a search answers with them: the columns
 * on screen, the newest rows first. Says how many there are, and whether there were more.
 */
export async function exportRumRows({
  tab,
  range,
  parsed,
  columns,
}: {
  tab: RumTab;
  range: { from: number; to: number };
  parsed: ParsedRumStatement;
  columns: readonly string[];
}): Promise<{ rows: number; more: boolean }> {
  const page = await rumApi.search({
    type: tab,
    from: range.from,
    to: range.to,
    limit: MAX_EXPORTED_ROWS,
    columns: [...columns],
    ...(parsed.filters.length > 0 ? { filters: parsed.filters } : {}),
    ...(parsed.freeText.length > 0 ? { freeText: parsed.freeText } : {}),
  });

  downloadCsv(`molesignal-rum-${tab}-${stamp()}.csv`, rumRowsToCsv(page.items, columns));

  return { rows: page.items.length, more: page.hasMore };
}
