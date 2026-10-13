import type { TFunction } from 'i18next';
import { ChevronLeft, ChevronRight } from 'lucide-react';

import type { StatusPageLanguage } from '@/api/statusPages';

import {
  type ArchiveMonthWindow,
  formatArchiveMonth,
} from './history';

const STEP =
  'grid h-[44px] w-[44px] shrink-0 place-items-center rounded-md bg-bg-2 text-tx-1 transition-colors enabled:hover:text-tx-0 enabled:focus-visible:text-tx-0 disabled:cursor-not-allowed disabled:text-tx-4 sm:h-9 sm:w-9';

/** The months of the past being looked at, and the steps to the ones before and after. */
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
    <div className="flex shrink-0 items-center justify-between gap-3">
      <button
        type="button"
        aria-label={copy('public.archive.previous_month')}
        disabled={!window.canPrevious}
        onClick={onPrevious}
        className={STEP}
      >
        <ChevronLeft aria-hidden className="h-4 w-4" strokeWidth={2.25} />
      </button>
      <p aria-live="polite" className="min-w-0 text-center text-sm font-strong tabular-nums text-tx-0 sm:min-w-52">
        {range}
      </p>
      <button
        type="button"
        aria-label={copy('public.archive.next_month')}
        disabled={!window.canNext}
        onClick={onNext}
        className={STEP}
      >
        <ChevronRight aria-hidden className="h-4 w-4" strokeWidth={2.25} />
      </button>
    </div>
  );
}
