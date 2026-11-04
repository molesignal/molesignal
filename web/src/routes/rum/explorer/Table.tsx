import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumField, RumRow } from '@/api/rum';
import { formatMicros, useTimeFormatter } from '@/lib/time';
import { gridMinWidth } from '@/routes/apm/frame/gridWidth';
import { cn } from '@/shell/lib/cn';
import { LONG_LIST_ROW_CLASS, longListRowStyle } from '@/shell/list/longList';

import { columnLayout, RumCell } from './cells';
import { columnLabel } from './labels';
import { isTroubled, type RumTab, rowId } from './model';

/** A row is one height, the line under it included, as a log line is: what an undrawn row reserves is what a drawn one takes. */
const ROW_HEIGHT = 32;
const ROW_STYLE = { minHeight: ROW_HEIGHT, ...longListRowStyle(ROW_HEIGHT - 1) };

/** When a row happened is the first column of every type. */
const TIME_TRACK = 'minmax(150px, 0.9fr)';

interface Clock {
  tz: string;
  format: ReturnType<typeof useTimeFormatter>['format'];
  dateFormat: ReturnType<typeof useTimeFormatter>['dateFormat'];
}

interface RumTableProps {
  tab: RumTab;
  /** The columns, as the reader arranged them. */
  columns: readonly RumField[];
  rows: readonly RumRow[];
  /** The row open in the drawer. */
  openId: string | null;
  onOpen: (row: RumRow, element: HTMLElement) => void;
  /** What stands in the rows' place when there are none to show. */
  empty?: React.ReactNode;
}

/**
 * The rows of the page as a table of one line each, in the columns the reader arranged. A click
 * on a row opens it. Rows that went wrong carry a mark down their left edge.
 */
export function RumTable({ tab, columns, rows, openId, onOpen, empty }: RumTableProps) {
  const { t, i18n } = useTranslation('rum');
  const fmt = useTimeFormatter();
  const clock = React.useMemo<Clock>(
    () => ({ tz: fmt.tz, format: fmt.format, dateFormat: fmt.dateFormat }),
    [fmt.dateFormat, fmt.format, fmt.tz],
  );
  const layouts = React.useMemo(() => columns.map(columnLayout), [columns]);
  const tracks = React.useMemo(() => [TIME_TRACK, ...layouts.map((layout) => layout.track)], [layouts]);
  const grid = tracks.join(' ');
  // The head and every row are as wide as their columns: past the pane's edge the table scrolls sideways as one.
  const minWidth = gridMinWidth(tracks);
  const listed = rows.length > 0;
  const language = i18n.resolvedLanguage ?? i18n.language;
  const listRef = React.useRef<HTMLDivElement>(null);

  // One stop in the tab order: the arrows walk the rows from there.
  const onKeyDown = React.useCallback((event: React.KeyboardEvent<HTMLButtonElement>) => {
    const step = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const items = [...(listRef.current?.querySelectorAll<HTMLButtonElement>('[data-rum-row]') ?? [])];
    const at = items.indexOf(event.currentTarget);
    items[Math.max(0, Math.min(items.length - 1, at + step))]?.focus();
  }, []);

  return (
    <div data-testid="rum-table" className="flex min-h-0 flex-1 flex-col">
      {/* The head scrolls with the rows: sideways they move together; down, it stays over them. */}
      <div ref={listRef} className={cn('min-h-0 scroll-pt-8', listed ? 'flex-1 overflow-auto' : 'shrink-0 overflow-hidden')}>
        <div
          data-rum-table-head=""
          className="sticky top-0 z-10 grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0"
          style={{ gridTemplateColumns: grid, minWidth }}
        >
          <span className="truncate">{t('explorer.columns.time')}</span>
          {columns.map((column, index) => (
            <span
              key={column.name}
              className={cn('truncate', layouts[index]?.align === 'end' && 'text-right')}
              title={column.name}
            >
              {columnLabel(t, column.name)}
            </span>
          ))}
        </div>
        {rows.map((row, index) => {
          const id = rowId(tab, row);

          return (
            <RumTableRow
              key={`${id}:${row.timestamp}:${index}`}
              tab={tab}
              row={row}
              id={id}
              columns={columns}
              layouts={layouts}
              grid={grid}
              minWidth={minWidth}
              clock={clock}
              language={language}
              open={id !== '' && id === openId}
              tabStop={openId ? id === openId : index === 0}
              onOpen={onOpen}
              onKeyDown={onKeyDown}
            />
          );
        })}
      </div>
      {!listed && <div className="min-h-0 flex-1 overflow-auto">{empty}</div>}
    </div>
  );
}

/** A row of the table, drawn again only when it changes itself: a page can hold thousands. */
const RumTableRow = React.memo(function RumTableRow({
  tab,
  row,
  id,
  columns,
  layouts,
  grid,
  minWidth,
  clock,
  language,
  open,
  tabStop,
  onOpen,
  onKeyDown,
}: {
  tab: RumTab;
  row: RumRow;
  id: string;
  columns: readonly RumField[];
  layouts: ReturnType<typeof columnLayout>[];
  grid: string;
  minWidth: string;
  clock: Clock;
  language: string;
  open: boolean;
  tabStop: boolean;
  onOpen: (row: RumRow, element: HTMLElement) => void;
  onKeyDown: (event: React.KeyboardEvent<HTMLButtonElement>) => void;
}) {
  const troubled = isTroubled(tab, row);

  return (
    <button
      type="button"
      data-rum-row=""
      data-rum-id={id}
      aria-pressed={open}
      tabIndex={tabStop ? 0 : -1}
      onClick={(event) => onOpen(row, event.currentTarget)}
      onKeyDown={onKeyDown}
      style={{ ...ROW_STYLE, gridTemplateColumns: grid, minWidth }}
      className={cn(
        'relative grid w-full items-center gap-3 border-b border-[var(--row-rule)] px-3 text-left font-sans text-xs',
        LONG_LIST_ROW_CLASS,
        open ? 'bg-indigo-dim' : 'hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none',
      )}
    >
      {/* How the row went, at a glance down the left edge. */}
      {troubled && <span aria-hidden="true" className="absolute inset-y-0 left-0 w-[3px] bg-red" />}
      <span className="truncate tabular-nums text-tx-0">
        {formatMicros(row.timestamp, clock.tz, clock.format, true, clock.dateFormat)}
      </span>
      {columns.map((column, index) => (
        <span
          key={column.name}
          className={cn('flex min-w-0', layouts[index]?.align === 'end' && 'justify-end')}
        >
          <RumCell field={column} value={row[column.name]} language={language} />
        </span>
      ))}
    </button>
  );
});
