import * as React from 'react';

import { cn } from '@/shell/lib/cn';
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

export interface DataTableColumn<T> {
  key: string;
  header: React.ReactNode;
  cell: (row: T) => React.ReactNode;
  className?: string;
  headerClassName?: string;
  width?: number | string;
}

export interface DataTableProps<T> {
  rows: T[];
  columns: DataTableColumn<T>[];
  rowKey: (row: T, index: number) => string;
  onRowClick?: (row: T) => void;
  isRowClickDisabled?: (row: T) => boolean;
  rowClickDisabledReason?: (row: T) => React.ReactNode;
  emptyLabel?: React.ReactNode;
  className?: string;
}

/**
 * Opinionated wrapper around the token-aware shadcn Table primitive
 * (`shell/ui/table.tsx`). Sticks the header, renders the empty state, and
 * collapses single-row click to a row-level handler.
 *
 * The visual language (row-height tracks `--row-height`, hover bg-bg-3,
 * selected bg-bg-4) flows from the primitive, not from this wrapper —
 * keep token decisions there.
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
}: DataTableProps<T>) {
  if (rows.length === 0) {
    return (
      <div
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
    <Table className={cn('font-strong', className)}>
      <TableHeader className="[&_th]:sticky [&_th]:top-0 [&_th]:bg-bg-1">
        <TableRow>
          {columns.map((c) => (
            <TableHead
              key={c.key}
              style={c.width ? { width: c.width } : undefined}
              className={c.headerClassName}
            >
              {c.header}
            </TableHead>
          ))}
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((row, index) => {
          const clickable = Boolean(onRowClick);
          const clickDisabled =
            clickable && Boolean(isRowClickDisabled?.(row));
          const activate = () => {
            if (!clickDisabled) onRowClick?.(row);
          };
          const tableRow = (
            <TableRow
              key={rowKey(row, index)}
              onClick={clickable && !clickDisabled ? activate : undefined}
              onKeyDown={
                clickable && !clickDisabled
                  ? (event) => {
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
              {columns.map((column) => (
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
            <Tooltip key={rowKey(row, index)}>
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
