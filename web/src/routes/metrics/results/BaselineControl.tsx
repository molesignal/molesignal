import { History, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { BaselinePicker } from '@/time/baseline/BaselinePicker';
import type { CompareBaseline, Period } from '@/time/baseline/periods';

/**
 * Whether the graph is laid over a baseline, and which: off, a button that
 * turns it on with the stretch before; on, the baseline as Profiles picks it
 * (the stretch before, a day or a week earlier, or a time range of its own)
 * and a button that takes it off.
 */
export function BaselineControl({
  value,
  comparison,
  resolved,
  failed,
  onChange,
}: {
  value: CompareBaseline | null;
  comparison: Period;
  resolved: Period | null;
  failed: boolean;
  onChange: (baseline: CompareBaseline | null) => void;
}) {
  const { t } = useTranslation('metrics');

  if (!value || !resolved) {
    return (
      <ChromeButton size="sm" onClick={() => onChange({ kind: 'previous' })} data-metrics-baseline-on="" className="h-11 sm:h-8">
        <History className="h-3.5 w-3.5" aria-hidden="true" />
        {t('explore.baseline.compare')}
      </ChromeButton>
    );
  }

  return (
    <span className="flex shrink-0 items-center gap-1" data-metrics-baseline={value.kind} data-failed={failed || undefined}>
      <BaselinePicker
        value={value}
        comparison={comparison}
        resolved={resolved}
        onChange={onChange}
        className="h-11 w-auto max-w-[240px] text-xs sm:h-8"
      />
      <button
        type="button"
        aria-label={t('explore.baseline.stop')}
        title={failed ? t('explore.baseline.failed') : t('explore.baseline.stop')}
        onClick={() => onChange(null)}
        className="grid h-8 w-8 shrink-0 place-items-center rounded-md text-tx-3 hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 focus-visible:text-tx-1"
      >
        <X className="h-3.5 w-3.5" aria-hidden="true" />
      </button>
    </span>
  );
}
