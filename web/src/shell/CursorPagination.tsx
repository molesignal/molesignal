import { ChevronLeft, ChevronRight } from 'lucide-react';

import { cn } from '@/shell/lib/cn';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

interface CursorPaginationProps {
  pageSize: number;
  pageSizeOptions: number[];
  hasPrevious: boolean;
  hasNext: boolean;
  pending?: boolean;
  ariaLabel: string;
  pageSizeAriaLabel: string;
  previousLabel: string;
  nextLabel: string;
  onPrevious: () => void;
  onNext: () => void;
  onPageSizeChange: (pageSize: number) => void;
  className?: string | undefined;
}

/**
 * Keyset pagination for high-volume, continuously written signal data. It
 * intentionally exposes no page number or last-page action because a cursor
 * response does not calculate an exact total.
 */
export function CursorPagination({
  pageSize,
  pageSizeOptions,
  hasPrevious,
  hasNext,
  pending = false,
  ariaLabel,
  pageSizeAriaLabel,
  previousLabel,
  nextLabel,
  onPrevious,
  onNext,
  onPageSizeChange,
  className,
}: CursorPaginationProps) {
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
            <SelectItem
              key={option}
              value={String(option)}
              className="font-sans text-xs"
            >
              {option}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <div className="ml-auto flex items-center gap-1">
        <button
          type="button"
          disabled={!hasPrevious || pending}
          onClick={onPrevious}
          className="flex h-11 items-center gap-1 rounded-md px-2 text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent disabled:hover:text-tx-2 lg:h-8"
        >
          <ChevronLeft className="h-3.5 w-3.5" aria-hidden="true" />
          <span>{previousLabel}</span>
        </button>
        <button
          type="button"
          disabled={!hasNext || pending}
          onClick={onNext}
          className="flex h-11 items-center gap-1 rounded-md px-2 text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent disabled:hover:text-tx-2 lg:h-8"
        >
          <span>{nextLabel}</span>
          <ChevronRight className="h-3.5 w-3.5" aria-hidden="true" />
        </button>
      </div>
    </nav>
  );
}
