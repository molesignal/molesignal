import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { LatencyBucket, RedSummary } from '@/api/apm';
import { cn } from '@/shell/lib/cn';

import { formatCount, formatDuration } from '../format';

/** The bounds of the rows, in microseconds: 1, 2 and 5 of each power of ten, from 1 µs to 5000 s. */
const EDGES = Array.from({ length: 10 }, (_, power) => [1, 2, 5].map((step) => step * 10 ** power)).flat();

/** Each percentile in an ink that warms toward the tail. */
const MARKS = [
  { label: 'P50', key: 'p50Time', tone: 'text-[var(--green-ink)]' },
  { label: 'P90', key: 'p90Time', tone: 'text-[var(--yellow-ink)]' },
  { label: 'P95', key: 'p95Time', tone: 'text-[var(--orange-ink)]' },
  { label: 'P99', key: 'p99Time', tone: 'text-[var(--red-ink)]' },
] as const;

interface Row {
  lower: number;
  upper: number;
  count: number;
}

/**
 * How the latencies of the requests spread: one row for each range of
 * latency (1, 2 and 5 of each power of ten), its bar as long as the requests
 * that took that long, drawn as a trace draws its spans, with the count,
 * the share and the percentiles that fall in it just past the bar's end. A
 * comparison is a dashed outline of the same range before, by share.
 */
export function LatencyDistribution({
  buckets,
  compared,
  red,
  apdexThreshold: apdexThresholdMicros,
  comparedLabel,
}: {
  buckets: readonly LatencyBucket[];
  compared?: readonly LatencyBucket[] | null | undefined;
  red: RedSummary;
  /** The threshold T the Apdex was read with, said beside it. */
  apdexThreshold?: number | undefined;
  /** What the dashed outline is of when it is not the stretch before: another environment. */
  comparedLabel?: string | undefined;
}) {
  const { t } = useTranslation('apm');
  const rows = React.useMemo(() => rowsOf(buckets), [buckets]);
  const total = rows.reduce((sum, row) => sum + row.count, 0);

  if (total === 0) return <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('charts.no_distribution')}</p>;

  const before = compared?.length ? countsByEdge(compared) : null;
  const totalBefore = before ? [...before.values()].reduce((sum, count) => sum + count, 0) : 0;
  const shareBefore = (row: Row) => (before && totalBefore > 0 ? (before.get(row.lower) ?? 0) / totalBefore : 0);
  const widest = Math.max(...rows.map((row) => row.count / total), ...rows.map(shareBefore), 0.0001);
  const marks = MARKS.flatMap((mark) => {
    const micros = red[mark.key];
    return micros == null ? [] : [{ ...mark, micros }];
  });
  let below = 0;

  return (
    <figure data-apm-latency-distribution="" className="px-3 pb-2 pt-2">
      <ol aria-label={t('charts.distribution')}>
        {rows.map((row) => {
          const share = row.count / total;
          below += row.count;
          const atOrBelow = below / total;
          const here = marks.filter((mark) => mark.micros >= row.lower && mark.micros < row.upper);
          const label = rangeLabel(row.lower, row.upper);
          const width = row.count > 0 ? Math.max(0.6, (share / widest) * 100) : 0;

          return (
            <li
              key={row.lower}
              data-apm-latency-row={label}
              title={t('charts.distribution_row', {
                range: label,
                count: formatCount(row.count),
                share: percent(share),
                below: percent(atOrBelow),
              })}
              className="type-micro grid h-[18px] grid-cols-[84px_minmax(0,1fr)] items-center gap-2 rounded-[3px] px-1 font-sans hover:bg-bg-2"
            >
              <span className="truncate text-right tabular-nums text-tx-2">{label}</span>
              {/* As the spans of a trace: the bar from the start, what it holds just past its end. */}
              <span className="relative mr-[140px] h-[10px]">
                {width > 0 && <span className="absolute inset-y-0 left-0 rounded-[3px] bg-[var(--chart-1)]" style={{ width: `${width}%` }} />}
                {before && shareBefore(row) > 0 && (
                  <span
                    aria-hidden
                    className="absolute inset-y-0 left-0 rounded-[3px] border border-dashed border-tx-3"
                    style={{ width: `${(shareBefore(row) / widest) * 100}%` }}
                  />
                )}
                {row.count > 0 && (
                  <span
                    className="pointer-events-none absolute top-1/2 flex -translate-y-1/2 items-center gap-1.5 whitespace-nowrap pl-1.5 tabular-nums"
                    style={{ left: `${Math.max(width, before ? (shareBefore(row) / widest) * 100 : 0)}%` }}
                  >
                    <span className="text-tx-1">{formatCount(row.count)}</span>
                    <span className="text-tx-3">{percent(share)}</span>
                    {here.map((mark) => (
                      <span key={mark.label} className={cn('font-strong', mark.tone)}>
                        {mark.label}
                      </span>
                    ))}
                  </span>
                )}
              </span>
            </li>
          );
        })}
      </ol>
      <figcaption className="type-micro mt-1.5 flex flex-wrap items-center gap-3 font-sans text-tx-2">
        {marks.map((mark) => (
          <span key={mark.label}>
            <span className={cn('font-strong', mark.tone)}>{mark.label}</span> <span className="tabular-nums text-tx-0">{formatDuration(mark.micros)}</span>
          </span>
        ))}
        {red.durationAverage != null && (
          <span>
            {t('catalog.columns.average')} <span className="tabular-nums text-tx-0">{formatDuration(red.durationAverage)}</span>
          </span>
        )}
        {red.apdex != null && (
          <span data-apm-apdex="">
            Apdex <span className="tabular-nums text-tx-0">{red.apdex.toFixed(2)}</span>
            {apdexThresholdMicros != null && <span className="text-tx-3"> {t('charts.apdex_threshold', { threshold: formatDuration(apdexThresholdMicros) })}</span>}
          </span>
        )}
        {before && <span className="text-tx-3">{comparedLabel ? t('charts.distribution_compared', { what: comparedLabel }) : t('charts.distribution_previous')}</span>}
      </figcaption>
    </figure>
  );
}

/** The count of each row, by its lower bound: a bucket goes to the row its middle falls in. */
function countsByEdge(buckets: readonly LatencyBucket[]): Map<number, number> {
  const counts = new Map<number, number>();

  for (const bucket of buckets) {
    if (bucket.count <= 0) continue;
    const middle = Math.sqrt(Math.max(1, bucket.lowerTime) * Math.max(1, bucket.upperTime));
    const index = Math.max(0, EDGES.findLastIndex((edge) => edge <= middle));
    const lower = EDGES[index]!;
    counts.set(lower, (counts.get(lower) ?? 0) + bucket.count);
  }

  return counts;
}

/** The rows from the fastest to the slowest that has requests, the empty ones between kept so the shape holds. */
export function rowsOf(buckets: readonly LatencyBucket[]): Row[] {
  const counts = countsByEdge(buckets);
  const filled = EDGES.map((edge, index) => ({ edge, index })).filter(({ edge }) => counts.has(edge));
  const first = filled[0]?.index;
  const last = filled.at(-1)?.index;
  if (first === undefined || last === undefined) return [];

  return EDGES.slice(first, last + 1).map((lower, offset) => ({
    lower,
    upper: EDGES[first + offset + 1] ?? lower * 2,
    count: counts.get(lower) ?? 0,
  }));
}

/** A bound as people write it: 500 µs, 20 ms, 2 s. */
function bound(micros: number): { value: number; unit: string } {
  if (micros < 1_000) return { value: micros, unit: 'µs' };
  if (micros < 1_000_000) return { value: micros / 1_000, unit: 'ms' };
  return { value: micros / 1_000_000, unit: 's' };
}

/** A row's range, its unit said once when both bounds share it: `50–100 ms`, `500 ms–1 s`. */
export function rangeLabel(lower: number, upper: number): string {
  const from = bound(lower);
  const to = bound(upper);
  return from.unit === to.unit ? `${from.value}–${to.value} ${to.unit}` : `${from.value} ${from.unit}–${to.value} ${to.unit}`;
}

function percent(share: number): string {
  return share >= 0.1 ? `${Math.round(share * 100)}%` : `${(share * 100).toFixed(1)}%`;
}
