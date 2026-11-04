import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import type { Trend } from '../model';

const WIDTH = 760;
const HEIGHT = 156;
const LEFT = 44;
const RIGHT = 8;
const TOP = 10;
const BOTTOM = 26;
const BAR = 32;
const DAY_MICROS = 24 * 60 * 60 * 1_000_000;

/** The colours of the versions of a stack, the oldest first; what is left over is drawn in grey. */
const INKS = ['var(--chart-1)', 'var(--chart-2)', 'var(--chart-3)', 'var(--chart-4)', 'var(--chart-5)'] as const;
const REST_INK = 'var(--tx-3)';

/** A step for a scale of three that is a round number: 1, 2, 2.5, 5 times a power of ten. */
function stepFor(widest: number): number {
  const wanted = Math.max(widest, 3) / 3;
  const power = 10 ** Math.floor(Math.log10(wanted));

  return ([1, 2, 2.5, 5, 10].map((unit) => unit * power).find((step) => step >= wanted) ?? 10 * power);
}

const inkOf = (index: number, version: string | null) => (version === '' ? REST_INK : (INKS[index % INKS.length] ?? REST_INK));

/** The key of the chart: a swatch and the version. */
export function TrendLegend({ trend }: { trend: Trend }) {
  const { t } = useTranslation('rum');

  return (
    <span className="ml-auto flex flex-wrap items-center gap-x-3.5 gap-y-1 font-sans text-xs text-tx-1">
      {trend.series.map((series, index) => (
        <span key={series.version ?? 'none'} className="inline-flex items-center gap-1.5">
          <span aria-hidden className="h-2.5 w-2.5 rounded-[2px]" style={{ background: inkOf(index, series.version) }} />
          {series.version === '' ? t('errors.trend.others') : (series.version ?? t('errors.trend.no_version'))}
        </span>
      ))}
    </span>
  );
}

/**
 * The sessions an issue met on each of the last days, stacked by the version they ran: the oldest
 * version at the foot of a bar. When the issue was resolved in the range, a dashed line marks the day.
 */
export function TrendChart({
  trend,
  resolvedAt,
  resolvedVersion,
  from,
  language,
}: {
  trend: Trend;
  resolvedAt: number | null;
  resolvedVersion: string | null;
  from: number;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const days = trend.days.length;
  const slot = (WIDTH - LEFT - RIGHT) / days;
  const stacked = trend.days.map((_, day) => trend.series.reduce((sum, series) => sum + (series.counts[day] ?? 0), 0));
  const step = stepFor(Math.max(0, ...stacked));
  const top = step * 3;
  const plot = HEIGHT - TOP - BOTTOM;
  const y = (value: number) => TOP + plot - (value / top) * plot;
  const label = new Intl.DateTimeFormat(language, { month: '2-digit', day: '2-digit' });

  const resolvedDay = resolvedAt === null ? -1 : Math.floor((resolvedAt - from) / DAY_MICROS);
  const marked = resolvedDay >= 0 && resolvedDay < days;
  const markerX = LEFT + (resolvedDay + 0.5) * slot;
  const total = stacked.reduce((sum, value) => sum + value, 0);

  return (
    <svg
      role="img"
      data-rum-trend-chart=""
      aria-label={t('errors.trend.aria', { count: total, days })}
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      className="block h-auto w-full"
    >
      <g className="stroke-[var(--row-rule)]" strokeWidth={1}>
        {[0, 1, 2, 3].map((line) => (
          <line key={line} x1={LEFT} x2={WIDTH - RIGHT} y1={y(line * step)} y2={y(line * step)} />
        ))}
      </g>
      <g className="fill-[var(--tx-3)] text-xs" textAnchor="end">
        {[0, 1, 2, 3].map((line) => (
          <text key={line} x={LEFT - 8} y={y(line * step) + 4}>
            {new Intl.NumberFormat(language).format(line * step)}
          </text>
        ))}
      </g>
      {trend.days.map((start, day) => {
        let floor = 0;

        return (
          <g key={start} data-rum-trend-day={day}>
            {trend.series.map((series, index) => {
              const count = series.counts[day] ?? 0;

              if (count === 0) return null;

              const rect = (
                <rect
                  key={series.version ?? 'none'}
                  x={LEFT + day * slot + (slot - BAR) / 2}
                  y={y(floor + count)}
                  width={BAR}
                  height={Math.max(1, y(floor) - y(floor + count))}
                  fill={inkOf(index, series.version)}
                />
              );

              floor += count;

              return rect;
            })}
          </g>
        );
      })}
      {marked && (
        <g data-rum-trend-resolved="">
          <line x1={markerX} x2={markerX} y1={TOP - 4} y2={TOP + plot} stroke="var(--green)" strokeWidth={1.5} strokeDasharray="4 4" />
          <text
            x={markerX + (resolvedDay > days / 2 ? -8 : 8)}
            y={TOP + 8}
            textAnchor={resolvedDay > days / 2 ? 'end' : 'start'}
            className="fill-[var(--green-ink)] text-xs"
          >
            {resolvedVersion ? t('errors.trend.resolved_in', { version: resolvedVersion }) : t('errors.trend.resolved')}
          </text>
        </g>
      )}
      <g className={cn('fill-[var(--tx-3)] text-xs')} textAnchor="middle">
        {trend.days.map((start, day) => (
          <text key={start} x={LEFT + (day + 0.5) * slot} y={HEIGHT - 8}>
            {label.format(new Date(start / 1_000))}
          </text>
        ))}
      </g>
    </svg>
  );
}
