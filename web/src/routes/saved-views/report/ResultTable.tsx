import * as React from 'react';

import { formatMicrosActive } from '@/lib/time';

/** The engine's own time column: microseconds since the epoch, in SQL and PromQL results alike. */
const TIMESTAMP_COLUMN = '_timestamp';

/** Column widths are counted in characters of the table's monospaced face. */
const MIN_COLUMN_CHARS = 4;
/** A long value wraps inside its column instead of taking the page from the others. */
const MAX_COLUMN_CHARS = 48;
/** What a cell adds around its text: padding on both sides and a rule. */
const CELL_CHROME_CHARS = 4;

/**
 * The rows a saved view returns, as a report prints them.
 *
 * Paper does not scroll, so the table is never wider than the page, whatever
 * the statement selects. Each column gets a share of the width in proportion
 * to the longest thing in it: a short value keeps its words whole, and a value
 * too long for its column wraps inside it.
 */
export function ResultTable({
  columns,
  rows,
  label,
}: {
  columns: readonly string[];
  rows: readonly (readonly unknown[])[];
  label: string;
}) {
  const texts = React.useMemo(
    () => rows.map((row) => columns.map((column, index) => cellText(column, row[index]))),
    [columns, rows],
  );
  const widths = React.useMemo(() => columnWidths(columns, texts), [columns, texts]);
  const total = widths.reduce((sum, width) => sum + width, 0);

  return (
    <table
      aria-label={label}
      // As wide as the columns ask for, and no wider than the page.
      style={{ width: `min(100%, ${total}ch)` }}
      className="table-fixed border-collapse border border-bd-0 font-mono text-xs [overflow-wrap:anywhere]"
    >
      <colgroup>
        {widths.map((width, index) => (
          <col key={index} style={{ width: `${(width / total) * 100}%` }} />
        ))}
      </colgroup>
      <thead className="bg-bg-2">
        <tr>
          {columns.map((column, index) => (
            // A statement can select the same name twice.
            <th
              key={`${index}:${column}`}
              scope="col"
              className="border-b border-r border-bd-0 px-[10px] py-[7px] text-left align-bottom font-semibold text-tx-2 last:border-r-0"
            >
              {column}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {texts.map((row, rowIndex) => (
          <tr key={rowIndex}>
            {row.map((text, columnIndex) => (
              <td
                key={columnIndex}
                className="whitespace-pre-wrap border-b border-r border-bd-0 px-[10px] py-[6px] align-top tabular-nums text-tx-0 last:border-r-0"
              >
                {text}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** A cell as it is read on paper: an instant as a time in the report's zone, anything else as returned. */
export function cellText(column: string, value: unknown): string {
  if (value === null || value === undefined) return '—';
  if (column === TIMESTAMP_COLUMN && typeof value === 'number') return formatMicrosActive(value);
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);

  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
}

/**
 * How wide each column asks to be, in characters: its longest line, the header
 * included, within bounds, plus what the cell adds around the text.
 */
export function columnWidths(
  columns: readonly string[],
  texts: readonly (readonly string[])[],
): number[] {
  return columns.map((column, index) => {
    let longest = column.length;
    for (const row of texts) longest = Math.max(longest, longestLine(row[index] ?? ''));

    return Math.min(Math.max(longest, MIN_COLUMN_CHARS), MAX_COLUMN_CHARS) + CELL_CHROME_CHARS;
  });
}

function longestLine(text: string): number {
  return text.split('\n').reduce((longest, line) => Math.max(longest, line.length), 0);
}
