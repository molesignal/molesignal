import { Diamond, ExternalLink } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { PrometheusExemplarSeries } from '@/api/query';
import { cn } from '@/shell/lib/cn';

const MAX_VISIBLE_MARKERS = 200;
const TRACE_ID_ALIASES = ['trace_id', 'traceid', 'trace.id', 'TraceID'];
const SPAN_ID_ALIASES = ['span_id', 'spanid', 'span.id', 'SpanID'];

export interface MetricExemplarPoint {
  id: string;
  timestampMicros: number;
  value: number;
  traceId?: string;
  spanId?: string;
  labels: Record<string, string>;
  seriesLabels: Record<string, string>;
}

export function flattenPrometheusExemplars(
  series: ReadonlyArray<PrometheusExemplarSeries>,
): MetricExemplarPoint[] {
  return series
    .flatMap((item) =>
      item.exemplars.map((exemplar) => {
        const timestampMicros = exemplar.timestamp * 1_000_000;
        const traceId = findLabel(exemplar.labels, TRACE_ID_ALIASES);
        const spanId = findLabel(exemplar.labels, SPAN_ID_ALIASES);
        return {
          id: [
            item.seriesLabels.__name__ ?? 'metric',
            timestampMicros,
            traceId ?? '',
            spanId ?? '',
            exemplar.value,
          ].join(':'),
          timestampMicros,
          value: exemplar.value,
          ...(traceId ? { traceId } : {}),
          ...(spanId ? { spanId } : {}),
          labels: exemplar.labels,
          seriesLabels: item.seriesLabels,
        };
      }),
    )
    .filter((point) => Number.isFinite(point.timestampMicros))
    .sort((left, right) => left.timestampMicros - right.timestampMicros);
}

export function ExemplarRail({
  series,
  fromMicros,
  toMicros,
  warning,
  error,
}: {
  series: ReadonlyArray<PrometheusExemplarSeries>;
  fromMicros: number;
  toMicros: number;
  warning?: string;
  error?: string;
}) {
  const { t } = useTranslation('metrics');
  const points = React.useMemo(
    () => flattenPrometheusExemplars(series),
    [series],
  );
  const visible = React.useMemo(
    () => evenlySample(points, MAX_VISIBLE_MARKERS),
    [points],
  );
  if (points.length === 0 && !warning && !error) return null;

  const span = Math.max(toMicros - fromMicros, 1);
  return (
    <section
      className="mt-2 rounded-md border border-bd-0 bg-bg-2/50 px-3 py-2"
      aria-label={t('explore.exemplars.title')}
    >
      <div className="flex min-w-0 items-center gap-2 text-xs">
        <Diamond className="h-3.5 w-3.5 shrink-0 text-purple-soft" aria-hidden />
        <span className="font-strong text-tx-1">
          {t('explore.exemplars.title')}
        </span>
        <span className="text-tx-3">
          {t('explore.exemplars.count', { count: points.length })}
        </span>
        {(warning || error) && (
          <span className={cn('ml-auto truncate', error ? 'text-orange-soft' : 'text-tx-3')}>
            {error ?? warning}
          </span>
        )}
      </div>
      {visible.length > 0 && (
        <div
          className="relative mt-2 h-10 rounded bg-bg-1"
          data-testid="metrics-exemplar-rail"
        >
          {visible.map((point, index) => {
            const left = Math.min(
              100,
              Math.max(0, ((point.timestampMicros - fromMicros) / span) * 100),
            );
            const title = exemplarTitle(point);
            const markerClass =
              'absolute grid h-5 w-5 -translate-x-1/2 place-items-center rounded text-purple-soft outline-none hover:bg-purple-dim hover:text-purple focus-visible:bg-purple-dim focus-visible:text-purple';
            const style = {
              left: `${left}%`,
              top: `${4 + (index % 2) * 16}px`,
            };
            return point.traceId ? (
              <Link
                key={`${point.id}:${index}`}
                to={`/traces/${encodeURIComponent(point.traceId)}`}
                className={markerClass}
                style={style}
                title={title}
                aria-label={t('explore.exemplars.open_trace', {
                  traceId: point.traceId,
                })}
              >
                <Diamond className="h-3 w-3 fill-current" aria-hidden />
              </Link>
            ) : (
              <span
                key={`${point.id}:${index}`}
                className={markerClass}
                style={style}
                title={title}
                role="img"
                aria-label={t('explore.exemplars.no_trace')}
              >
                <Diamond className="h-3 w-3" aria-hidden />
              </span>
            );
          })}
        </div>
      )}
      {points.some((point) => point.traceId) && (
        <div className="mt-1.5 flex items-center gap-1 text-type-micro text-tx-3">
          <ExternalLink className="h-3 w-3" aria-hidden />
          {t('explore.exemplars.hint')}
        </div>
      )}
    </section>
  );
}

function findLabel(
  labels: Record<string, string>,
  aliases: ReadonlyArray<string>,
): string | undefined {
  for (const alias of aliases) {
    const exact = labels[alias];
    if (exact) return exact;
    const lower = alias.toLowerCase();
    const entry = Object.entries(labels).find(
      ([name, value]) => name.toLowerCase() === lower && Boolean(value),
    );
    if (entry) return entry[1];
  }
  return undefined;
}

function evenlySample<T>(items: ReadonlyArray<T>, limit: number): T[] {
  if (items.length <= limit) return [...items];
  if (limit <= 1) return items.length > 0 ? [items[items.length - 1]!] : [];
  const last = items.length - 1;
  return Array.from({ length: limit }, (_, index) => {
    const sourceIndex = Math.round((index * last) / (limit - 1));
    return items[sourceIndex]!;
  });
}

function exemplarTitle(point: MetricExemplarPoint): string {
  const metric = point.seriesLabels.__name__ ?? 'metric';
  const labels = Object.entries(point.labels)
    .map(([name, value]) => `${name}=${value}`)
    .join(', ');
  return `${metric} · ${new Date(point.timestampMicros / 1000).toLocaleString()} · ${point.value}${labels ? ` · ${labels}` : ''}`;
}
