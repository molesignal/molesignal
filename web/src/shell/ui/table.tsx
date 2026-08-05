import * as React from 'react';

import { cn } from '@/shell/lib/cn';

/**
 * Table — Phase 4 token-aware shadcn primitive.
 *
 * Brief mandates (M0.3):
 *   - row-height tracks --row-height (density-aware: 24/28/34px)
 *   - cell padding tracks --row-pad-x / --row-pad-y
 *   - hover row uses bg-bg-3 (the dedicated hover layer)
 *   - selected row uses bg-bg-4 (the active layer)
 *   - sort indicator uses indigo accent (brand)
 *
 * Apply `data-state="selected"` to any TableRow to flag it as the
 * keyboard / multi-select active row. Apply `aria-sort="ascending" |
 * "descending"` to TableHead to surface the sort indicator.
 */

const Table = React.forwardRef<HTMLTableElement, React.HTMLAttributes<HTMLTableElement>>(
  ({ className, ...props }, ref) => (
    <div className="relative w-full overflow-auto">
      <table
        ref={ref}
        className={cn(
          'w-full caption-bottom border-collapse font-sans text-sm text-tx-1 tabular-nums',
          className,
        )}
        {...props}
      />
    </div>
  ),
);
Table.displayName = 'Table';

const TableHeader = React.forwardRef<
  HTMLTableSectionElement,
  React.HTMLAttributes<HTMLTableSectionElement>
>(({ className, ...props }, ref) => (
  <thead ref={ref} className={cn('[&_tr]:border-b [&_tr]:border-bd-0', className)} {...props} />
));
TableHeader.displayName = 'TableHeader';

const TableBody = React.forwardRef<
  HTMLTableSectionElement,
  React.HTMLAttributes<HTMLTableSectionElement>
>(({ className, ...props }, ref) => (
  <tbody ref={ref} className={cn('[&_tr:last-child]:border-0', className)} {...props} />
));
TableBody.displayName = 'TableBody';

const TableFooter = React.forwardRef<
  HTMLTableSectionElement,
  React.HTMLAttributes<HTMLTableSectionElement>
>(({ className, ...props }, ref) => (
  <tfoot
    ref={ref}
    className={cn('border-t border-bd-0 bg-bg-2 font-strong [&>tr]:last:border-b-0', className)}
    {...props}
  />
));
TableFooter.displayName = 'TableFooter';

const TableRow = React.forwardRef<HTMLTableRowElement, React.HTMLAttributes<HTMLTableRowElement>>(
  ({ className, ...props }, ref) => (
    <tr
      ref={ref}
      className={cn(
        'h-row border-b border-bd-0',
        'transition-colors duration-fast ease-default',
        'hover:bg-bg-3',
        // selected row: bg-bg-4 + an inline indigo rail flush left, mirroring
        // the sidebar active-state language. Pair with role="row" and
        // data-state="selected" from the caller's list-keyboard logic.
        'data-[state=selected]:bg-bg-4',
        'data-[state=selected]:relative data-[state=selected]:before:absolute data-[state=selected]:before:left-0 data-[state=selected]:before:top-0 data-[state=selected]:before:h-full data-[state=selected]:before:w-0.5 data-[state=selected]:before:bg-indigo',
        className,
      )}
      {...props}
    />
  ),
);
TableRow.displayName = 'TableRow';

const TableHead = React.forwardRef<
  HTMLTableCellElement,
  React.ThHTMLAttributes<HTMLTableCellElement>
>(({ className, ...props }, ref) => (
  <th
    ref={ref}
    className={cn(
      'px-row-pad-x py-row-pad-y text-left align-middle',
      'font-sans text-xs font-strong uppercase tracking-wider text-tx-3',
      // aria-sort surfaces the sorted column with an indigo top border.
      '[&[aria-sort=ascending]]:border-t-2 [&[aria-sort=ascending]]:border-t-indigo [&[aria-sort=ascending]]:text-tx-0',
      '[&[aria-sort=descending]]:border-t-2 [&[aria-sort=descending]]:border-t-indigo [&[aria-sort=descending]]:text-tx-0',
      '[&:has([role=checkbox])]:pr-0 [&>[role=checkbox]]:translate-y-[2px]',
      className,
    )}
    {...props}
  />
));
TableHead.displayName = 'TableHead';

const TableCell = React.forwardRef<
  HTMLTableCellElement,
  React.TdHTMLAttributes<HTMLTableCellElement>
>(({ className, ...props }, ref) => (
  <td
    ref={ref}
    className={cn(
      'px-row-pad-x py-row-pad-y align-middle',
      '[&:has([role=checkbox])]:pr-0 [&>[role=checkbox]]:translate-y-[2px]',
      className,
    )}
    {...props}
  />
));
TableCell.displayName = 'TableCell';

const TableCaption = React.forwardRef<
  HTMLTableCaptionElement,
  React.HTMLAttributes<HTMLTableCaptionElement>
>(({ className, ...props }, ref) => (
  <caption ref={ref} className={cn('mt-4 text-xs text-tx-2', className)} {...props} />
));
TableCaption.displayName = 'TableCaption';

export { Table, TableBody, TableCaption, TableCell, TableFooter, TableHead, TableHeader, TableRow };
