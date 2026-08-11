import type { TFunction } from 'i18next';
import { ChevronLeft, ChevronRight } from 'lucide-react';

import type { StatusPageLanguage } from '@/api/statusPages';

import {
  type ArchiveMonthWindow,
  formatArchiveMonth,
} from './publicStatusHistory';

export function ArchiveMonthNavigator({
  window,
  language,
  copy,
  onPrevious,
  onNext,
}: {
  window: ArchiveMonthWindow;
  language: StatusPageLanguage;
  copy: TFunction;
  onPrevious: () => void;
  onNext: () => void;
}) {
  const start = formatArchiveMonth(window.startIndex, language);
  const end = formatArchiveMonth(window.endIndex, language);
  const range =
    window.startIndex === window.endIndex
      ? start
      : copy('public.archive.month_range', { start, end });

  return (
    <div className="mt-6 flex items-center justify-between gap-4 sm:justify-end">
      <button
        type="button"
        aria-label={copy('public.archive.previous_month')}
        disabled={!window.canPrevious}
        onClick={onPrevious}
        className="grid h-11 w-11 shrink-0 place-items-center rounded-md border border-bd-0 text-tx-2 transition-colors enabled:hover:bg-bg-1 enabled:hover:text-tx-0 enabled:focus-visible:bg-bg-1 disabled:cursor-not-allowed disabled:text-tx-4"
      >
        <ChevronLeft aria-hidden className="h-5 w-5" strokeWidth={2.25} />
      </button>
      <p
        aria-live="polite"
        className="min-w-0 text-center text-base font-display-strong tabular-nums text-tx-0 sm:min-w-64"
      >
        {range}
      </p>
      <button
        type="button"
        aria-label={copy('public.archive.next_month')}
        disabled={!window.canNext}
        onClick={onNext}
        className="grid h-11 w-11 shrink-0 place-items-center rounded-md border border-bd-0 text-tx-2 transition-colors enabled:hover:bg-bg-1 enabled:hover:text-tx-0 enabled:focus-visible:bg-bg-1 disabled:cursor-not-allowed disabled:text-tx-4"
      >
        <ChevronRight aria-hidden className="h-5 w-5" strokeWidth={2.25} />
      </button>
    </div>
  );
}
