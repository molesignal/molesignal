import { ArrowDown, ArrowUp, ArrowUpDown } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { isNestedControlClick } from '@/shell/lib/nestedControl';
import { Checkbox } from '@/shell/ui/checkbox';
import { Skeleton } from '@/shell/ui/skeleton';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

import { nextSort, sortRows, type SortDirection, type TableSort, type TableValue } from './table/model';

export interface DataTableColumn<T> {
  key: string;
  header: React.ReactNode;
  cell: (row: T) => React.ReactNode;
  className?: string;
  headerClassName?: string;
  width?: number | string;
  /** The plain value of the cell: what an export writes and, with `sortable`, what the column sorts by. */
  value?: (row: T) => TableValue;
  /** Where the order is not the words': a status by how much it wants attention, a time by the time. */
  sortValue?: (row: T) => TableValue;
  sortable?: boolean;
  /** In a file only: what a cell on screen already shows together with something else. */
  fileOnly?: boolean;
}

export interface DataTableSelection<T> {
  selected: ReadonlySet<string>;
  onChange: (next: ReadonlySet<string>) => void;
  isSelectable?: (row: T) => boolean;
}

export interface DataTableProps<T> {
  rows: readonly T[];
  columns: readonly DataTableColumn<T>[];
  rowKey: (row: T, index: number) => string;
  onRowClick?: (row: T) => void;
  isRowClickDisabled?: (row: T) => boolean;
  rowClickDisabledReason?: (row: T) => React.ReactNode;
  emptyLabel?: React.ReactNode;
  className?: string;
  /** The rows are on their way: the head stays, the cells are bars of about their size. */
  loading?: boolean;
  /** How the list opens. Without it, in the order it was given. */
  defaultSort?: TableSort;
  /**
   * A list that is paged sorts before it pages: `useTableSort` holds the order and
   * sorts the rows, and the table only draws it.
   */
  sort?: TableSort | null;
  onSortChange?: (sort: TableSort | null) => void;
  /** Ticks a box on each row; `useTableSelection` holds the state. */
  selection?: DataTableSelection<T>;
  /**
   * Columns keep the widths they ask for and the one that asks for none takes the
   * rest, so a long name cannot push the others away; narrower than this, the table scrolls.
   */
  fixedMinWidth?: number;
}

/** Bars of different lengths, so a column of them reads as words and not as a grid. */
const LOADING_WIDTHS = ['72%', '48%', '60%', '36%', '54%', '66%'];
const LOADING_ROWS = 6;

function SortableHead({
  label,
  direction,
  onToggle,
}: {
  label: React.ReactNode;
  direction: SortDirection | null;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onToggle}
      className={cn(
        'group/sort relative inline-flex max-w-full items-center gap-1 rounded-sm font-strong after:absolute after:-inset-y-1 after:inset-x-0 after:content-[""] hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none',
      )}
    >
      <span className="truncate">{label}</span>
      {direction === 'desc' ? (
        <ArrowDown aria-hidden className="h-3 w-3 shrink-0" />
      ) : direction === 'asc' ? (
        <ArrowUp aria-hidden className="h-3 w-3 shrink-0" />
      ) : (
        <ArrowUpDown
          aria-hidden
          className="h-3 w-3 shrink-0 text-tx-3 opacity-0 group-hover/sort:opacity-100 group-focus-visible/sort:opacity-100"
        />
      )}
    </button>
  );
}

/**
 * Opinionated wrapper around the token-aware shadcn Table primitive
 * (`shell/ui/table.tsx`). Sticks the header, renders the empty state, and
 * collapses single-row click to a row-level handler.
 *
 * The visual language (row-height tracks `--row-height`, hover bg-bg-3,
 * selected bg-bg-4) flows from the primitive, not from this wrapper —
 * keep token decisions there.
 *
 * What a list may need beyond its rows is opt-in, column by column: `sortable`
 * with a `value` sorts in the browser, `selection` ticks rows (`TableBulkBar`
 * acts on them), `value` feeds `TableExportButton`.
 */
export function DataTable<T>({
  rows,
  columns,
  rowKey,
  onRowClick,
  isRowClickDisabled,
  rowClickDisabledReason,
  emptyLabel = 'No rows',
  className,
  loading = false,
  defaultSort,
  sort: heldSort,
  onSortChange,
  selection,
  fixedMinWidth,
}: DataTableProps<T>) {
  const { t } = useTranslation('common');
  const shownColumns = columns.filter((column) => !column.fileOnly);
  const tableProps = {
    className: cn(fixedMinWidth !== undefined && 'table-fixed', className),
    style: fixedMinWidth !== undefined ? { minWidth: fixedMinWidth } : undefined,
  };
  const [ownSort, setOwnSort] = React.useState<TableSort | null>(defaultSort ?? null);
  const sortedByCaller = heldSort !== undefined;
  const sort = sortedByCaller ? heldSort : ownSort;

  const sortedColumn = sort ? columns.find((column) => column.key === sort.key) : undefined;
  const orderOf = sortedColumn?.sortValue ?? sortedColumn?.value;
  const shownRows = React.useMemo(
    () => (!sortedByCaller && sort && orderOf ? sortRows(rows, orderOf, sort.direction) : rows),
    [orderOf, rows, sort, sortedByCaller],
  );
  const toggleSort = (key: string) => {
    if (sortedByCaller) onSortChange?.(nextSort(sort, key));
    else setOwnSort((current) => nextSort(current, key));
  };

  const selectableKeys = selection
    ? rows.flatMap((row, index) => ((selection.isSelectable?.(row) ?? true) ? [rowKey(row, index)] : []))
    : [];
  const selectedCount = selection ? selectableKeys.filter((key) => selection.selected.has(key)).length : 0;

  const head = (
    <TableHeader className="[&_th]:sticky [&_th]:top-0">
      <TableRow>
        {selection && (
          <TableHead className="w-9">
            <Checkbox
              aria-label={t('table.select_all')}
              className="relative after:absolute after:-inset-2 after:content-['']"
              disabled={loading || selectableKeys.length === 0}
              checked={
                selectableKeys.length > 0 && selectedCount === selectableKeys.length
                  ? true
                  : selectedCount > 0
                    ? 'indeterminate'
                    : false
              }
              onCheckedChange={(checked) => {
                // These rows only: what is ticked on another page of the list stays ticked.
                const next = new Set(selection.selected);
                for (const key of selectableKeys) {
                  if (checked === true) next.add(key);
                  else next.delete(key);
                }
                selection.onChange(next);
              }}
            />
          </TableHead>
        )}
        {shownColumns.map((c) => {
          const direction = sort?.key === c.key ? sort.direction : null;
          return (
            <TableHead
              key={c.key}
              style={c.width ? { width: c.width } : undefined}
              className={c.headerClassName}
              aria-sort={direction === 'asc' ? 'ascending' : direction === 'desc' ? 'descending' : undefined}
            >
              {c.sortable && (c.sortValue ?? c.value) ? (
                <SortableHead
                  label={c.header}
                  direction={direction}
                  onToggle={() => toggleSort(c.key)}
                />
              ) : (
                c.header
              )}
            </TableHead>
          );
        })}
      </TableRow>
    </TableHeader>
  );

  if (loading) {
    return (
      <Table data-data-table data-data-table-loading aria-busy="true" {...tableProps}>
        {head}
        <TableBody>
          {Array.from({ length: LOADING_ROWS }, (_, rowIndex) => (
            <TableRow key={rowIndex} className="hover:bg-transparent">
              {selection && <TableCell className="w-9" />}
              {shownColumns.map((column, columnIndex) => (
                <TableCell key={column.key} className={column.className}>
                  {/* A column of buttons has no text to stand in for. */}
                  {column.key !== 'actions' && (
                    <Skeleton
                      className="inline-block h-3 rounded-sm align-middle"
                      style={{ width: LOADING_WIDTHS[(rowIndex + columnIndex * 2) % LOADING_WIDTHS.length] }}
                    />
                  )}
                </TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    );
  }

  if (rows.length === 0) {
    return (
      <div
        data-data-table-empty
        className={cn(
          'flex h-32 items-center justify-center rounded-md border border-bd-0 bg-bg-1 font-sans text-xs text-tx-2',
          className,
        )}
      >
        {emptyLabel}
      </div>
    );
  }
  return (
    <Table data-data-table {...tableProps}>
      {head}
      <TableBody>
        {shownRows.map((row, index) => {
          const key = rowKey(row, index);
          const rowSelected = selection?.selected.has(key) ?? false;
          const clickable = Boolean(onRowClick);
          const clickDisabled =
            clickable && Boolean(isRowClickDisabled?.(row));
          const activate = () => {
            if (!clickDisabled) onRowClick?.(row);
          };
          const tableRow = (
            <TableRow
              key={key}
              data-state={rowSelected ? 'selected' : undefined}
              // A click on a control in a cell (a reference, a link) is the control's, not the row's.
              onClick={
                clickable && !clickDisabled
                  ? (event) => {
                      if (!isNestedControlClick(event)) activate();
                    }
                  : undefined
              }
              onKeyDown={
                clickable && !clickDisabled
                  ? (event) => {
                      // A key on a control in a cell (a box, a link) is the control's, not the row's.
                      if (event.target !== event.currentTarget) return;

                      if (event.key === 'Enter' || event.key === ' ') {
                        event.preventDefault();
                        activate();
                      }
                    }
                  : undefined
              }
              tabIndex={clickable && !clickDisabled ? 0 : undefined}
              aria-disabled={clickDisabled || undefined}
              className={cn(
                clickable && !clickDisabled && 'cursor-pointer',
                clickDisabled &&
                  'cursor-not-allowed text-tx-3 hover:!bg-transparent',
              )}
            >
              {selection && (
                <TableCell className="w-9">
                  {(selection.isSelectable?.(row) ?? true) && (
                    <Checkbox
                      aria-label={t('table.select_row')}
                      className="relative after:absolute after:-inset-2 after:content-['']"
                      checked={rowSelected}
                      onCheckedChange={(checked) => {
                        const next = new Set(selection.selected);
                        if (checked === true) next.add(key);
                        else next.delete(key);
                        selection.onChange(next);
                      }}
                    />
                  )}
                </TableCell>
              )}
              {shownColumns.map((column) => (
                <TableCell
                  key={column.key}
                  className={cn(
                    'overflow-hidden text-ellipsis whitespace-nowrap',
                    column.className,
                  )}
                >
                  {column.cell(row)}
                </TableCell>
              ))}
            </TableRow>
          );
          const reason = clickDisabled
            ? rowClickDisabledReason?.(row)
            : undefined;
          if (!reason) return tableRow;
          return (
            <Tooltip key={key}>
              <TooltipTrigger asChild>{tableRow}</TooltipTrigger>
              <TooltipContent side="top" className="max-w-xs leading-relaxed">
                {reason}
              </TooltipContent>
            </Tooltip>
          );
        })}
      </TableBody>
    </Table>
  );
}
