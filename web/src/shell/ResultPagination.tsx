import {
  ChevronLeft,
  ChevronRight,
  ChevronsLeft,
  ChevronsRight,
} from 'lucide-react';

import { cn } from '@/shell/lib/cn';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

interface ResultPaginationProps {
  page: number;
  pageCount: number;
  pageSize: number;
  pageSizeOptions: number[];
  pageLabel: string;
  ariaLabel: string;
  pageSizeAriaLabel: string;
  firstAriaLabel: string;
  previousAriaLabel: string;
  nextAriaLabel: string;
  lastAriaLabel: string;
  onPageChange: (page: number) => void;
  onPageSizeChange: (pageSize: number) => void;
  className?: string | undefined;
}

/**
 * Compact pagination for query workbenches. The fixed 44px footer aligns with
 * CollapsibleSidePanel status rows so fields and results share one baseline.
 */
export function ResultPagination({
  page,
  pageCount,
  pageSize,
  pageSizeOptions,
  pageLabel,
  ariaLabel,
  pageSizeAriaLabel,
  firstAriaLabel,
  previousAriaLabel,
  nextAriaLabel,
  lastAriaLabel,
  onPageChange,
  onPageSizeChange,
  className,
}: ResultPaginationProps) {
  const safePageCount = Math.max(1, pageCount);
  const safePage = Math.min(Math.max(1, page), safePageCount);

  return (
    <nav
      aria-label={ariaLabel}
      className={cn(
        'flex h-11 w-full shrink-0 items-center border-t border-bd-0 bg-bg-1 px-2 font-sans text-xs',
        className,
      )}
    >
      <Select
        value={String(pageSize)}
        onValueChange={(value) => onPageSizeChange(Number(value))}
      >
        <SelectTrigger
          aria-label={pageSizeAriaLabel}
          className="h-11 w-auto min-w-[3.25rem] border-0 bg-transparent px-1.5 font-sans text-xs font-strong text-tx-1 shadow-none hover:border-0 hover:bg-transparent focus-visible:bg-transparent focus-visible:text-tx-0 data-[state=open]:border-0 data-[state=open]:bg-transparent lg:h-8"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {pageSizeOptions.map((option) => (
            <SelectItem key={option} value={String(option)} className="font-sans text-xs">
              {option}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <div className="ml-auto flex items-center gap-0">
        <button
          type="button"
          aria-label={firstAriaLabel}
          disabled={safePage <= 1}
          onClick={() => onPageChange(1)}
          className="grid h-11 w-11 place-items-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent disabled:hover:text-tx-2 lg:h-8 lg:w-7"
        >
          <ChevronsLeft className="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          aria-label={previousAriaLabel}
          disabled={safePage <= 1}
          onClick={() => onPageChange(safePage - 1)}
          className="grid h-11 w-11 place-items-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent disabled:hover:text-tx-2 lg:h-8 lg:w-7"
        >
          <ChevronLeft className="h-3.5 w-3.5" />
        </button>
        <span
          aria-live="polite"
          className="min-w-[48px] whitespace-nowrap px-0.5 text-center font-mono tabular-nums text-tx-1"
        >
          {pageLabel}
        </span>
        <button
          type="button"
          aria-label={nextAriaLabel}
          disabled={safePage >= safePageCount}
          onClick={() => onPageChange(safePage + 1)}
          className="grid h-11 w-11 place-items-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent disabled:hover:text-tx-2 lg:h-8 lg:w-7"
        >
          <ChevronRight className="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          aria-label={lastAriaLabel}
          disabled={safePage >= safePageCount}
          onClick={() => onPageChange(safePageCount)}
          className="grid h-11 w-11 place-items-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent disabled:hover:text-tx-2 lg:h-8 lg:w-7"
        >
          <ChevronsRight className="h-3.5 w-3.5" />
        </button>
      </div>
    </nav>
  );
}
