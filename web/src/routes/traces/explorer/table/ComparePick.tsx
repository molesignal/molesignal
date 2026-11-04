import { X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/shell/ui/button';
import { formatTraceDurationMs } from '@/viz/trace/duration';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import type { DisplayTrace } from '../types';

/** The letter each picked trace stands under in the comparison, in the order they were picked. */
export const PICK_LETTERS = ['A', 'B'] as const;

/**
 * The bar over the table while two traces are being picked to compare: the
 * ones picked, under the letter each stands under, and the comparison once
 * there are two.
 */
export function ComparePickBar({
  picked,
  onUnpick,
  onCompare,
  onCancel,
}: {
  picked: readonly DisplayTrace[];
  onUnpick: (trace: DisplayTrace) => void;
  onCompare: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation('traces');

  return (
    <div
      role="region"
      aria-label={t('explore.compare.bar')}
      data-testid="trace-compare-pick"
      className="mx-3 mb-[4px] flex min-h-9 flex-wrap items-center gap-x-3 gap-y-1 rounded-md bg-[var(--indigo-tint)] px-3 py-1 font-sans text-xs"
    >
      <span className="shrink-0 text-tx-1">{t(picked.length < 2 ? 'explore.compare.hint' : 'explore.compare.ready')}</span>
      {picked.map((trace, index) => (
        <span key={trace.id} className="flex min-w-0 max-w-[320px] items-center gap-1.5" data-picked-trace={trace.id}>
          <span className="shrink-0 font-strong text-indigo">{PICK_LETTERS[index]}</span>
          <TraceOperationName operation={trace.op} className="min-w-0 text-tx-0" />
          <span className="shrink-0 tabular-nums text-tx-2">{formatTraceDurationMs(trace.durationMs)}</span>
          <button
            type="button"
            aria-label={t('explore.compare.unpick', { operation: trace.op })}
            title={t('explore.compare.unpick', { operation: trace.op })}
            onClick={() => onUnpick(trace)}
            className="grid h-5 w-5 shrink-0 place-items-center rounded text-tx-3 hover:bg-indigo/15 hover:text-tx-0 focus-visible:bg-indigo/15 focus-visible:outline-none"
          >
            <X className="h-3 w-3" />
          </button>
        </span>
      ))}
      <span className="ml-auto flex shrink-0 items-center gap-1.5">
        <Button size="sm" className="h-7" disabled={picked.length < 2} onClick={onCompare} data-testid="trace-compare-picked">
          {t('explore.compare.compare')}
        </Button>
        <Button variant="ghost" size="sm" className="h-7" onClick={onCancel}>
          {t('explore.compare.cancel')}
        </Button>
      </span>
    </div>
  );
}
