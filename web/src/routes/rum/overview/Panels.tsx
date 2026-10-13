import { AlertTriangle, ArrowRight } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ExperienceGrade } from '@/api/rum';
import type {
  ExperienceBucket,
  OverviewDimensionShare,
  OverviewFrequentError,
  OverviewSlowPage,
  SatisfactionCounts,
} from '@/api/rum/overview';
import { cn } from '@/shell/lib/cn';
import { useTimeStore } from '@/stores/useTimeStore';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { formatDurationMs, formatShare } from '../_helpers';
import { RumSectionHeader } from '../RumLayout';

export function ExperienceTrend({
  buckets,
  range,
}: {
  buckets: ExperienceBucket[];
  range: { from_micros: number; to_micros: number };
}) {
  const { t } = useTranslation('rum');
  const setWindow = useTimeStore((state) => state.setWindow);
  return (
    <section className="h-full rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)]">
      <RumSectionHeader
        title={t('overview.experience_trend')}
        description={t('overview.experience_trend_description')}
      />
      <div className="pt-4">
        <TimeSeriesChart
          series={[
            {
              id: 'experience-good',
              name: t('experience.good'),
              color: 'var(--green)',
              data: buckets.map((bucket) => bucket.good),
              timestamps: buckets.map((bucket) => bucket.start),
            },
            {
              id: 'experience-needs',
              name: t('experience.needs_improvement'),
              color: 'var(--yellow)',
              data: buckets.map((bucket) => bucket.needs),
              timestamps: buckets.map((bucket) => bucket.start),
            },
            {
              id: 'experience-poor',
              name: t('experience.poor'),
              color: 'var(--red)',
              data: buckets.map((bucket) => bucket.poor),
              timestamps: buckets.map((bucket) => bucket.start),
            },
          ]}
          xDomain={[range.from_micros, range.to_micros]}
          height={220}
          ariaLabel={t('overview.experience_trend')}
          options={{
            drawStyle: 'bar',
            stackMode: 'normal',
            showPoints: 'never',
            legendMode: 'list',
            legendStats: [],
            leftAxis: { min: 0 },
          }}
          onRangeSelect={({ from, to }) =>
            setWindow({
              mode: 'absolute',
              from: new Date(from / 1000).toISOString(),
              to: new Date(to / 1000).toISOString(),
            })
          }
        />
      </div>
    </section>
  );
}

export function SatisfactionPanel({
  counts,
}: {
  counts: SatisfactionCounts;
}) {
  const { t } = useTranslation('rum');
  const total = Math.max(1, counts.total);
  const items: Array<{ grade: ExperienceGrade; count: number; color: string }> = [
    {
      grade: 'good',
      count: counts.good,
      color: 'bg-green',
    },
    {
      grade: 'needs_improvement',
      count: counts.needsImprovement,
      color: 'bg-yellow',
    },
    {
      grade: 'poor',
      count: counts.poor,
      color: 'bg-red',
    },
  ];
  return (
    <section className="h-full rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)]">
      <RumSectionHeader
        title={t('overview.satisfaction')}
        description={t('overview.satisfaction_description')}
      />
      <div className="grid gap-4 pb-1 pt-4">
        {items.map((item) => {
          const share = item.count / total;
          return (
            <div key={item.grade}>
              <div className="flex items-center justify-between text-xs">
                <span className="font-strong text-tx-1">
                  {t(`experience.${item.grade}`)}
                </span>
                <span className="tabular-nums text-tx-3">
                  {(share * 100).toFixed(1)}% · {item.count}
                </span>
              </div>
              <div className="mt-2 h-2 overflow-hidden rounded-full bg-bg-3">
                <div
                  className={cn('h-full rounded-full', item.color)}
                  style={{ width: `${share * 100}%` }}
                />
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}

/** The height of a row of either ranked list of the overview: they are read across as well as down. */
const RANKED_ROW_HEIGHT = 'h-[46px]';

export function SlowPagesPanel({ pages }: { pages: OverviewSlowPage[] }) {
  const { t } = useTranslation('rum');
  const max = Math.max(...pages.map((page) => page.p75), 1);
  return (
    <section className="h-full rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)]">
      <RumSectionHeader
        title={t('overview.slowest_pages')}
        description={t('overview.slowest_pages_description')}
        action={
          <Link
            to="/rum/pages"
            className="text-xs font-strong text-[var(--blue-ink)] hover:text-tx-0"
          >
            {t('overview.view_pages')} →
          </Link>
        }
      />
      {pages.length === 0 ? (
        <EmptyRow label={t('performance.no_page_data')} />
      ) : (
        // The two lists stand side by side, so a row is the same height in both: 46px, which
        // holds the taller of the two (a message over its line of detail) with 4px to spare.
        // A floor of 62px under a row of 32px left a third of each row empty.
        <div data-rum-ranked-list className="mt-2 space-y-0.5">
          {pages.slice(0, 5).map((page) => (
            <div
              key={page.page}
              data-rum-ranked-row
              className={cn(
                'grid grid-cols-[minmax(0,1fr)_90px_90px] items-center gap-4 rounded-md px-2 hover:bg-[var(--control-surface)]',
                RANKED_ROW_HEIGHT,
              )}
            >
              <div className="min-w-0">
                <div className="truncate text-sm font-strong text-tx-0">
                  {page.page}
                </div>
                <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-bg-3">
                  <div
                    className={cn(
                      'h-full rounded-full',
                      page.grade === 'poor'
                        ? 'bg-red'
                        : page.grade === 'needs_improvement'
                          ? 'bg-yellow'
                          : 'bg-green',
                    )}
                    style={{ width: `${Math.max(5, (page.p75 / max) * 100)}%` }}
                  />
                </div>
              </div>
              <span className="text-right font-mono text-xs font-strong text-tx-0">
                {formatDurationMs(page.p75)}
              </span>
              {/* An error rate is said where there is one: "0.0% error rate" on every row said nothing. */}
              <span data-rum-page-errors className="text-right text-xs text-tx-3">
                {page.errorRate > 0 ? `${formatShare(page.errorRate)} ${t('overview.error_rate')}` : ''}
              </span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

export function FrequentErrorsPanel({
  errors,
}: {
  errors: OverviewFrequentError[];
}) {
  const { t } = useTranslation('rum');
  return (
    <section className="h-full rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)]">
      <RumSectionHeader
        title={t('overview.frequent_errors')}
        description={t('overview.frequent_errors_description')}
        action={
          <Link
            to="/rum/errors"
            className="text-xs font-strong text-[var(--blue-ink)] hover:text-tx-0"
          >
            {t('overview.view_errors')} →
          </Link>
        }
      />
      {errors.length === 0 ? (
        <EmptyRow label={t('errors.empty_title')} />
      ) : (
        <div data-rum-ranked-list className="mt-2 space-y-0.5">
          {errors.map((error) => (
            <Link
              key={error.fingerprint}
              to={`/rum/errors/view/${encodeURIComponent(error.fingerprint)}`}
              data-rum-ranked-row
              className={cn(
                'group flex items-center gap-3 rounded-md px-2 hover:bg-[var(--control-surface)] focus-visible:bg-[var(--control-surface)]',
                RANKED_ROW_HEIGHT,
              )}
            >
              <AlertTriangle aria-hidden className="h-4 w-4 shrink-0 text-red-soft" />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-strong text-tx-0">
                  {error.message}
                </span>
                <span className="mt-0.5 block text-xs text-tx-3">
                  {error.users} {t('overview.users_unit')} · {error.version ?? '—'}
                </span>
              </span>
              <ArrowRight
                aria-hidden
                className="h-4 w-4 text-tx-3 transition-transform group-hover:translate-x-0.5"
              />
            </Link>
          ))}
        </div>
      )}
    </section>
  );
}

export function DimensionPanel({
  title,
  description,
  rows,
}: {
  title: string;
  description: string;
  rows: OverviewDimensionShare[];
}) {
  const { t } = useTranslation('rum');
  return (
    <section className="h-full rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)]">
      <RumSectionHeader title={title} description={description} />
      {rows.length === 0 ? (
        <EmptyRow label={t('performance.no_dimension_data')} />
      ) : (
        <div className="grid gap-3 py-4">
          {rows.map((row) => (
            <div key={row.label} className="grid grid-cols-[140px_1fr_64px] items-center gap-3">
              <span className="truncate text-xs font-strong text-tx-1">{row.label}</span>
              <span className="h-1.5 overflow-hidden rounded-full bg-bg-3">
                <span
                  className="block h-full rounded-full bg-indigo"
                  style={{ width: `${row.share * 100}%` }}
                />
              </span>
              <span className="text-right text-xs tabular-nums text-tx-3">
                {(row.share * 100).toFixed(1)}%
              </span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function EmptyRow({ label }: { label: string }) {
  return (
    <div className="grid min-h-40 place-items-center text-sm text-tx-3">
      {label}
    </div>
  );
}
