import { AlertTriangle, ArrowRight } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type {
  ErrorRow,
  ExperienceGrade,
  SessionRow,
} from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { useTimeStore } from '@/stores/useTimeStore';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { formatDurationMs } from '../_helpers';
import { RumSectionHeader } from '../RumLayout';
import type {
  DimensionShare,
  OverviewMetrics,
  SlowPage,
} from './model';

export function ExperienceTrend({
  sessions,
  range,
}: {
  sessions: SessionRow[];
  range: { from_micros: number; to_micros: number };
}) {
  const { t } = useTranslation('rum');
  const setWindow = useTimeStore((state) => state.setWindow);
  const buckets = bucketSessions(sessions, 12, range);
  return (
    <section>
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

export function CoreWebVitalsPanel({
  metrics,
}: {
  metrics: OverviewMetrics;
}) {
  const { t } = useTranslation('rum');
  const items = [
    {
      key: 'lcp',
      value: metrics.lcpP75,
      formatted: formatDurationMs(metrics.lcpP75),
      grade: durationGrade(metrics.lcpP75, 2_500, 4_000),
    },
    {
      key: 'inp',
      value: metrics.inpP75,
      formatted: formatDurationMs(metrics.inpP75),
      grade: durationGrade(metrics.inpP75, 200, 500),
    },
    {
      key: 'cls',
      value: metrics.clsP75,
      formatted: metrics.clsP75 > 0 ? metrics.clsP75.toFixed(3) : '—',
      grade: durationGrade(metrics.clsP75, 0.1, 0.25),
    },
  ];
  return (
    <section>
      <RumSectionHeader
        title={t('overview.core_web_vitals')}
        description={t('overview.core_web_vitals_description')}
      />
      <div className="grid gap-px bg-bd-0 sm:grid-cols-3">
        {items.map((item) => (
          <div key={item.key} className="bg-bg-0 px-4 py-5">
            <div className="flex items-center justify-between gap-3">
              <span className="text-xs font-strong text-tx-3">
                {t(`overview.vitals.${item.key}`)} P75
              </span>
              <span
                className={cn(
                  'h-2 w-2 rounded-full',
                  item.grade === 'good' && 'bg-green',
                  item.grade === 'needs_improvement' && 'bg-yellow',
                  item.grade === 'poor' && 'bg-red',
                  item.grade === 'unknown' && 'bg-tx-3',
                )}
              />
            </div>
            <div className="mt-2 font-mono text-xl font-strong text-tx-0">
              {item.formatted}
            </div>
            <div className="mt-1 text-xs text-tx-3">
              {item.value > 0
                ? t(`experience.${item.grade}`)
                : t('experience.unknown')}
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}

export function SatisfactionPanel({ sessions }: { sessions: SessionRow[] }) {
  const { t } = useTranslation('rum');
  const total = Math.max(1, sessions.length);
  const items: Array<{ grade: ExperienceGrade; count: number; color: string }> = [
    {
      grade: 'good',
      count: sessions.filter((row) => row.experience === 'good').length,
      color: 'bg-green',
    },
    {
      grade: 'needs_improvement',
      count: sessions.filter((row) => row.experience === 'needs_improvement').length,
      color: 'bg-yellow',
    },
    {
      grade: 'poor',
      count: sessions.filter((row) => row.experience === 'poor').length,
      color: 'bg-red',
    },
  ];
  return (
    <section>
      <RumSectionHeader
        title={t('overview.satisfaction')}
        description={t('overview.satisfaction_description')}
      />
      <div className="grid gap-4 py-5">
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

export function SlowPagesPanel({ pages }: { pages: SlowPage[] }) {
  const { t } = useTranslation('rum');
  const max = Math.max(...pages.map((page) => page.p75), 1);
  return (
    <section>
      <RumSectionHeader
        title={t('overview.slowest_pages')}
        description={t('overview.slowest_pages_description')}
        action={
          <Link
            to="/rum/pages"
            className="text-xs font-strong text-blue-soft hover:text-tx-0"
          >
            {t('overview.view_pages')} →
          </Link>
        }
      />
      {pages.length === 0 ? (
        <EmptyRow label={t('performance.no_page_data')} />
      ) : (
        <div className="divide-y divide-bd-0">
          {pages.slice(0, 5).map((page) => (
            <div
              key={page.page}
              className="grid min-h-[62px] grid-cols-[minmax(0,1fr)_90px_90px] items-center gap-4 py-2.5"
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
              <span className="text-right text-xs text-tx-3">
                {(page.errorRate * 100).toFixed(1)}% {t('overview.error_rate')}
              </span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

export function FrequentErrorsPanel({ errors }: { errors: ErrorRow[] }) {
  const { t } = useTranslation('rum');
  return (
    <section>
      <RumSectionHeader
        title={t('overview.frequent_errors')}
        description={t('overview.frequent_errors_description')}
        action={
          <Link
            to="/rum/errors"
            className="text-xs font-strong text-blue-soft hover:text-tx-0"
          >
            {t('overview.view_errors')} →
          </Link>
        }
      />
      {errors.length === 0 ? (
        <EmptyRow label={t('errors.empty_title')} />
      ) : (
        <div className="divide-y divide-bd-0">
          {errors.map((error) => (
            <Link
              key={error.fingerprint}
              to={`/rum/errors/view/${encodeURIComponent(error.fingerprint)}`}
              className="group flex min-h-[62px] items-center gap-3 py-2.5 hover:bg-bg-2 focus-visible:bg-bg-2"
            >
              <AlertTriangle aria-hidden className="h-4 w-4 shrink-0 text-red-soft" />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-strong text-tx-0">
                  {error.message}
                </span>
                <span className="mt-1 block text-xs text-tx-3">
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
  rows: DimensionShare[];
}) {
  const { t } = useTranslation('rum');
  return (
    <section>
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

function durationGrade(
  value: number,
  needsThreshold: number,
  poorThreshold: number,
): ExperienceGrade {
  if (value <= 0) return 'unknown';
  if (value > poorThreshold) return 'poor';
  if (value > needsThreshold) return 'needs_improvement';
  return 'good';
}

function bucketSessions(
  sessions: SessionRow[],
  count: number,
  range: { from_micros: number; to_micros: number },
) {
  const width = Math.max(1, (range.to_micros - range.from_micros) / count);
  const buckets = Array.from({ length: count }, (_, index) => ({
    start: range.from_micros + width * index,
    good: 0,
    needs: 0,
    poor: 0,
  }));
  for (const session of sessions) {
    const timestamp = session.started_at_micros ?? range.from_micros;
    const index = Math.min(
      count - 1,
      Math.max(0, Math.floor((timestamp - range.from_micros) / width)),
    );
    const bucket = buckets[index];
    if (!bucket) continue;
    if (session.experience === 'good') bucket.good += 1;
    else if (session.experience === 'needs_improvement') bucket.needs += 1;
    else if (session.experience === 'poor') bucket.poor += 1;
  }
  return buckets;
}
