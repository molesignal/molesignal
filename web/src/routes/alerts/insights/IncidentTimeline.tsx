import type { TFunction } from 'i18next';

import type { Incident } from '@/types/alerting';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

type AlertsTranslation = TFunction<'alerts'>;

interface TimelineBucket {
  startMs: number;
  endMs: number;
  count: number;
  label: string;
  tooltipLabel: string;
}

export function buildIncidentTimeline(
  incidents: readonly Pick<Incident, 'created_at'>[],
  windowSecs: number,
  nowMs = Date.now(),
  locale = 'zh-CN',
): TimelineBucket[] {
  const hourly = windowSecs <= 86_400;
  const rangeStartMs = nowMs - windowSecs * 1_000;
  const bucketStart = hourly ? startOfLocalHour : startOfLocalDay;
  const nextBucket = hourly ? addHour : addDay;
  const firstBucket = bucketStart(rangeStartMs);
  const lastBucket = bucketStart(nowMs);
  const counts = new Map<number, number>();

  for (const incident of incidents) {
    const createdAtMs = incident.created_at / 1_000;
    if (!Number.isFinite(createdAtMs) || createdAtMs < rangeStartMs || createdAtMs > nowMs) continue;
    const key = bucketStart(createdAtMs);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }

  const buckets: TimelineBucket[] = [];
  for (let cursor = firstBucket; cursor <= lastBucket; cursor = nextBucket(cursor)) {
    const endMs = nextBucket(cursor);
    buckets.push({
      startMs: cursor,
      endMs,
      count: counts.get(cursor) ?? 0,
      label: formatAxisLabel(cursor, hourly, locale),
      tooltipLabel: formatTooltipLabel(cursor, endMs, hourly, locale),
    });
  }
  return buckets.filter((bucket) => bucket.count > 0);
}

export function IncidentTimeline({
  incidents,
  windowSecs,
  locale,
  t,
}: {
  incidents: readonly Incident[];
  windowSecs: number;
  locale: string;
  t: AlertsTranslation;
}) {
  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const rangeEndMs = Date.now();
  const rangeStartMs = rangeEndMs - windowSecs * 1_000;
  const buckets = buildIncidentTimeline(incidents, windowSecs, rangeEndMs, locale);
  const maxCount = Math.max(0, ...buckets.map((bucket) => bucket.count));
  const busiest = buckets.filter((bucket) => bucket.count === maxCount && maxCount > 0);
  const peakSummary = t('insights.hourly.peak_summary', {
    count: maxCount,
    period: busiest.map((bucket) => bucket.label).join('、'),
    timezone,
  });
  const hasChartData = buckets.length > 0;
  const chartValues = hasChartData ? buckets.map((bucket) => bucket.count) : [0, 0];
  const chartTimestamps = hasChartData
    ? buckets.map((bucket) => (bucket.startMs + bucket.endMs) / 2)
    : [rangeStartMs, rangeEndMs];

  return (
    <section aria-labelledby="insights-hourly">
      <div className="flex flex-col gap-1 sm:flex-row sm:items-start sm:justify-between sm:gap-6">
        <div>
          <h3 id="insights-hourly" className="font-sans text-base font-display-strong text-tx-0">
            {t('insights.hourly.title')}
          </h3>
          <p className="mt-0.5 font-sans text-xs text-tx-3">
            {t('insights.hourly.description', { timezone })}
          </p>
        </div>
        {maxCount > 0 && (
          <p className="shrink-0 font-sans text-xs tabular-nums text-tx-2">{peakSummary}</p>
        )}
      </div>
      <div className="mt-3 h-[164px]">
        <TimeSeriesChart
          series={[
            {
              name: t('insights.hourly.series_name'),
              color: hasChartData ? 'var(--chart-1)' : 'transparent',
              data: chartValues,
              timestamps: chartTimestamps,
              unit: 'events',
            },
          ]}
          xDomain={[rangeStartMs, rangeEndMs]}
          timezone={timezone}
          height="100%"
          showLegend={false}
          ariaLabel={t('insights.hourly.chart_label', { summary: peakSummary })}
          options={{
            drawStyle: 'bar',
            compactAxes: true,
            leftAxis: { min: 0, unit: 'events' },
            ...(hasChartData ? {} : { tooltipMode: 'hidden' as const }),
          }}
        />
      </div>
    </section>
  );
}

function startOfLocalHour(value: number): number {
  const date = new Date(value);
  date.setMinutes(0, 0, 0);
  return date.getTime();
}

function startOfLocalDay(value: number): number {
  const date = new Date(value);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

function addHour(value: number): number {
  const date = new Date(value);
  date.setHours(date.getHours() + 1);
  return date.getTime();
}

function addDay(value: number): number {
  const date = new Date(value);
  date.setDate(date.getDate() + 1);
  return date.getTime();
}

function formatAxisLabel(value: number, hourly: boolean, locale: string): string {
  const date = new Date(value);
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  const dateLabel = locale.toLowerCase().startsWith('zh')
    ? `${date.getMonth() + 1}月${date.getDate()}日`
    : `${month}/${day}`;
  if (!hourly) return dateLabel;

  const hour = String(date.getHours()).padStart(2, '0');
  return `${dateLabel} ${hour}:00`;
}

function formatTooltipLabel(startMs: number, endMs: number, hourly: boolean, locale: string): string {
  const formatter = new Intl.DateTimeFormat(locale, hourly
    ? { year: 'numeric', month: 'long', day: 'numeric', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }
    : { year: 'numeric', month: 'long', day: 'numeric' });
  if (!hourly) return formatter.format(startMs);
  return formatter.formatRange(new Date(startMs), new Date(endMs));
}
