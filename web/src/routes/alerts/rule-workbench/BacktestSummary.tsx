import { useTranslation } from 'react-i18next';

import { useTimeFormatter } from '@/lib/time';
import type { SeverityThreshold } from '@/types/alerting';

import type { RuleBacktest } from './backtest';

/** The most times named: the rest of a noisy rule is on the chart. */
const MAX_TIMES_NAMED = 3;

/** What the chart above it shows, in words: how often the rule would have fired, and when. */
export function BacktestSummary({
  backtest,
  threshold,
}: {
  backtest: RuleBacktest;
  threshold: SeverityThreshold;
}) {
  const { t } = useTranslation('alerts');
  const time = useTimeFormatter();
  const fired = backtest.firing.length;

  return (
    <div data-rule-backtest className="mt-3 font-sans text-sm text-tx-1">
      <p>
        {fired > 0
          ? t('workbench.preview.backtest_fires', {
              count: fired,
              times: backtest.firing
                .slice(0, MAX_TIMES_NAMED)
                .map((span) => time.micros(span.from, false))
                .join(', ')
                .concat(fired > MAX_TIMES_NAMED ? ' …' : ''),
            })
          : t('workbench.preview.backtest_quiet')}
      </p>
      {backtest.nearMisses > 0 && (
        <p className="mt-1 text-tx-2">
          {t('workbench.preview.backtest_near', {
            count: backtest.nearMisses,
            periods: threshold.forPeriods,
          })}
        </p>
      )}
      {(fired > 0 || backtest.pending.length > 0) && (
        <ul className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-tx-3">
          <li className="flex items-center gap-1.5">
            <span aria-hidden className="h-2.5 w-2.5 rounded-sm bg-[var(--yellow-fill)]" />
            {t('workbench.preview.span_pending')}
          </li>
          <li className="flex items-center gap-1.5">
            <span aria-hidden className="h-2.5 w-2.5 rounded-sm bg-[var(--red-fill)]" />
            {t('workbench.preview.span_firing')}
          </li>
        </ul>
      )}
    </div>
  );
}
