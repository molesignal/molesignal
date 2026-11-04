import { Repeat2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import type { Retention, RetentionPeriod } from '@/api/rum';
import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { useRetention } from './useAnalytics';
import { formatCount, formatPercent } from '../overview/format';
import { scopeFilters } from '../overview/model';
import { CardState, OverviewCard } from '../overview/OverviewCard';
import { CONTROL } from '../overview/ScopeRow';
import type { ScopeContext } from '../ScopedFrame';

const PERIODS = [7, 14, 30] as const;
const DEFAULT_PERIODS = 7;

/** The day a period begins, as the server counts days: of UTC, whatever the clock of the reader. */
const utcDay = (micros: number): string => new Date(micros / 1_000).toISOString().slice(0, 10);

/** How strongly a share of the cohort that came back is tinted. */
const tint = (share: number): string => `color-mix(in srgb, var(--chart-1) ${Math.round(8 + Math.min(1, share) * 62)}%, transparent)`;

/** The share of all the people of the cohorts that could have been seen `later` periods on that were. */
export function averages(retention: Retention): Array<number | null> {
  return Array.from({ length: retention.periods }, (_, index) => {
    const cohorts = retention.cohorts.filter((cohort) => cohort.returned[index] !== null && cohort.returned[index] !== undefined);
    const people = cohorts.reduce((sum, cohort) => sum + cohort.users, 0);

    return people > 0 ? cohorts.reduce((sum, cohort) => sum + (cohort.returned[index] ?? 0), 0) / people : null;
  });
}

function Matrix({ retention, language }: { retention: Retention; language: string }) {
  const { t } = useTranslation('rum');
  const mean = averages(retention);
  const unit = retention.period === 'week' ? 'w' : 'd';

  return (
    <div className="overflow-x-auto">
      <table data-rum-retention="" className="w-full min-w-[560px] border-collapse font-sans text-xs">
        <thead>
          <tr className="text-tx-3">
            <th scope="col" className="px-3 py-1.5 text-left font-body">
              {t(retention.period === 'week' ? 'analytics.retention.cohort_week' : 'analytics.retention.cohort_day')}
            </th>
            <th scope="col" className="px-3 py-1.5 text-right font-body">
              {t('analytics.retention.people')}
            </th>
            {Array.from({ length: retention.periods }, (_, index) => (
              <th key={index + 1} scope="col" className="px-1 py-1.5 text-center font-body tabular-nums">
                {`+${index + 1}${unit}`}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {retention.cohorts.map((cohort) => (
            <tr key={cohort.startTime} data-rum-retention-cohort={cohort.startTime} className="border-t border-[var(--row-rule)]">
              <th scope="row" className="whitespace-nowrap px-3 py-1.5 text-left font-strong text-tx-0">
                {utcDay(cohort.startTime)}
              </th>
              <td className="px-3 py-1.5 text-right tabular-nums text-tx-1">{formatCount(cohort.users, language)}</td>
              {cohort.returned.map((count, index) => {
                const share = count === null || cohort.users === 0 ? null : count / cohort.users;

                return (
                  <td
                    key={index}
                    data-rum-retention-cell={`${cohort.startTime}:${index + 1}`}
                    title={count === null ? undefined : t('analytics.retention.cell', { count, total: cohort.users })}
                    className={cn('px-1 py-1.5 text-center tabular-nums', share === null ? 'text-tx-4' : 'text-tx-0')}
                    style={share === null ? undefined : { background: tint(share) }}
                  >
                    {share === null ? '—' : formatPercent(share)}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr className="border-t border-bd-0 text-tx-1">
            <th scope="row" className="px-3 py-1.5 text-left font-strong">
              {t('analytics.retention.average')}
            </th>
            <td />
            {mean.map((share, index) => (
              <td key={index} data-rum-retention-average={index + 1} className="px-1 py-1.5 text-center font-strong tabular-nums">
                {share === null ? '—' : formatPercent(share)}
              </td>
            ))}
          </tr>
        </tfoot>
      </table>
    </div>
  );
}

/**
 * Who came back: the people first seen in a day or a week, and the share of them seen again in each
 * of the periods after. A person is told by their user id, or the anonymous id the SDK keeps.
 */
export function RetentionView({ context }: { context: ScopeContext }) {
  const { t } = useTranslation('rum');
  const { scope, range, language, window, setWindow } = context;
  const [params, setParams] = useSearchParams();

  const period: RetentionPeriod = params.get('period') === 'week' ? 'week' : 'day';
  const periods = (PERIODS as readonly number[]).includes(Number(params.get('periods'))) ? Number(params.get('periods')) : DEFAULT_PERIODS;

  const change = (name: string, value: string | null) =>
    setParams(
      (current) => {
        const next = new URLSearchParams(current);

        if (value) next.set(name, value);
        else next.delete(name);

        return next;
      },
      { replace: true },
    );

  const request = React.useMemo(
    () => ({ from: range.from, to: range.to, filters: scopeFilters(scope), period, periods }),
    [period, periods, range, scope],
  );

  const read = useRetention(request);
  const nobody = read.isSuccess && !read.data?.unavailable && read.data.cohorts.length === 0;
  // A range as short as a day has no period after the first that has ended: nothing to say yet.
  const tooShort =
    read.isSuccess &&
    !read.data?.unavailable &&
    read.data.cohorts.length > 0 &&
    read.data.cohorts.every((cohort) => cohort.returned.every((count) => count === null));

  return (
    <div data-rum-analytics="retention" className="space-y-[12px]">
      <OverviewCard icon={Repeat2} title={t('analytics.retention.title')} description={t('analytics.retention.description')} testId="retention" tone="text-[var(--chart-6)]">
        <div className="flex flex-wrap items-center gap-2 border-b border-bd-0 px-3 py-2">
          <FittedSelect
            aria-label={t('analytics.retention.period')}
            data-rum-retention-period=""
            value={period}
            onValueChange={(value) => change('period', value === 'week' ? 'week' : null)}
            options={[
              { value: 'day', label: t('analytics.retention.by_day') },
              { value: 'week', label: t('analytics.retention.by_week') },
            ]}
            maxWidth={200}
            className={cn(CONTROL, 'font-strong text-tx-1')}
          />
          <FittedSelect
            aria-label={t('analytics.retention.periods')}
            data-rum-retention-periods=""
            value={String(periods)}
            onValueChange={(value) => change('periods', Number(value) === DEFAULT_PERIODS ? null : value)}
            options={PERIODS.map((count) => ({ value: String(count), label: t(period === 'week' ? 'analytics.retention.weeks_after' : 'analytics.retention.days_after', { count }) }))}
            maxWidth={200}
            className={cn(CONTROL, 'text-tx-1')}
          />
        </div>
        <CardState query={read} empty={nobody} emptyTitle={t('analytics.retention.nobody')}>
          {read.data && <Matrix retention={read.data} language={language} />}
        </CardState>
        {tooShort && (
          <div data-rum-retention-short="" className="flex flex-wrap items-center gap-3 border-t border-bd-0 px-3 py-2.5 font-sans text-xs text-tx-2">
            <span className="min-w-0 flex-1">{t('analytics.retention.too_short')}</span>
            <ChromeButton onClick={() => setWindow(widenTimeWindow(window))}>{t('overview.empty.widen')}</ChromeButton>
          </div>
        )}
        {nobody && (
          <div className="flex justify-center pb-4">
            <ChromeButton onClick={() => setWindow(widenTimeWindow(window))}>{t('overview.empty.widen')}</ChromeButton>
          </div>
        )}
      </OverviewCard>
    </div>
  );
}
