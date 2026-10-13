import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { formatMicrosActive } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import {
  type DiffTotals,
  formatChange,
  formatChangePct,
  formatValue,
} from '@/viz/profiles/flamebearer';

import { type Period, periodsOverlap } from './periods';

/**
 * What a comparison is of, and where it is set: the period looked at and the
 * baseline it is set against, each with the control that chooses it, the time
 * it spans, how many profiles it holds and what they add up to. The change
 * between the two closes the statement.
 *
 * Until the comparison has answered, only the two spans are known.
 */
export function CompareStatement({
  comparison,
  baseline,
  comparisonControl,
  baselineControl,
  counts,
  totals,
  units,
  className,
}: {
  comparison: Period;
  baseline: Period;
  /** The controls that choose each period: any two stretches of time can be compared. */
  comparisonControl: React.ReactNode;
  baselineControl: React.ReactNode;
  counts?: { comparison: number; baseline: number } | undefined;
  totals?: DiffTotals | undefined;
  units?: string | undefined;
  className?: string | undefined;
}) {
  const { t } = useTranslation('profiles');
  const comparable = Boolean(counts && counts.comparison > 0 && counts.baseline > 0);
  const tone = !totals || totals.delta === 0 ? 'none' : totals.delta > 0 ? 'more' : 'less';

  const row = (role: 'comparison' | 'baseline', period: Period, control: React.ReactNode) => {
    const count = counts?.[role];
    const total = totals?.[role];

    return (
      <tr data-compare-period={role}>
        <th
          scope="row"
          className={cn(
            'whitespace-nowrap py-[3px] pr-[12px] text-left font-strong',
            role === 'comparison' ? 'text-tx-0' : 'text-tx-1',
          )}
        >
          {t(`compare.${role}`)}
        </th>
        {/* One column: the two controls are as wide as the wider of them. */}
        <td data-compare-control className="py-[3px] pr-[12px]">
          {control}
        </td>
        <td data-compare-range className="whitespace-nowrap py-[3px] pr-[16px] tabular-nums text-tx-1">
          {formatMicrosActive(period.from, false)} – {formatMicrosActive(period.to, false)}
        </td>
        <td className="whitespace-nowrap py-[3px] pr-[16px] tabular-nums text-tx-2">
          {count === undefined ? '' : t('compare.period_profiles', { count })}
        </td>
        <td className="whitespace-nowrap py-[3px] text-right tabular-nums text-tx-1">
          {count && total !== undefined && units !== undefined ? formatValue(total, units) : ''}
        </td>
      </tr>
    );
  };

  return (
    <section
      data-compare-statement
      aria-label={t('compare.statement_aria')}
      className={cn(
        'rounded-md bg-[var(--functional-surface)] px-[12px] py-[8px] font-sans text-xs [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      <div className="flex min-w-0 flex-wrap items-center gap-x-[24px] gap-y-[8px]">
        <table className="border-collapse">
          <tbody>
            {row('comparison', comparison, comparisonControl)}
            {row('baseline', baseline, baselineControl)}
          </tbody>
        </table>

        {comparable && totals && units !== undefined ? (
          <div data-compare-overall className="ml-auto text-right">
            <div className="text-tx-2">{t('compare.overall')}</div>
            <div
              className={cn(
                'mt-[2px] text-sm font-strong tabular-nums',
                tone === 'more' && 'text-[var(--red-ink)]',
                tone === 'less' && 'text-[var(--green-ink)]',
                tone === 'none' && 'text-tx-1',
              )}
            >
              {formatChange(totals.delta, units)}
              {totals.deltaPct !== null ? ` · ${formatChangePct(totals.deltaPct)}` : null}
            </div>
          </div>
        ) : null}
      </div>

      {periodsOverlap(comparison, baseline) ? (
        <p data-compare-overlap className="mt-[6px] text-[var(--yellow-ink)]">
          {t('compare.overlap')}
        </p>
      ) : null}

      {comparable && counts && counts.comparison !== counts.baseline ? (
        <p data-compare-unequal className="mt-[6px] text-tx-2">
          {t('compare.unequal_counts', { comparison: counts.comparison, baseline: counts.baseline })}
        </p>
      ) : null}
    </section>
  );
}
