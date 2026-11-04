import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { tracesRed, type TraceRedResponse, type TraceRedScope } from '@/api/traceRed';
import type { TraceFilter } from '@/api/web';
import { quoteTraceValue } from '@/routes/traces/urlState';
import { cn } from '@/shell/lib/cn';
import { Sparkline } from '@/shell/readout/Sparkline';

import { formatTraceDurationNs } from '../duration';

/** How far either side of the span the figures are read. */
const AROUND_MICROS = 15 * 60 * 1_000_000;
const BAR_MICROS = 60 * 1_000_000;

interface Scope {
  key: 'service' | 'operation';
  filters: TraceFilter[];
  /** A service's requests are the spans it took in; an operation's, every span of it. */
  scope: TraceRedScope;
}

/**
 * How the span's service and its operation were doing around the span: the
 * requests, the failures and how long the slowest twentieth took, minute by
 * minute over half an hour, read as the RED view reads them. The view itself
 * is a link away.
 */
export function SpanMetrics({ service, operation, startNs }: { service: string; operation: string; startNs: number }) {
  const { t } = useTranslation('traces');
  const centre = Math.floor(startNs / 1_000);
  const to = Math.min(Date.now() * 1_000, centre + AROUND_MICROS);
  const from = Math.min(centre, to) - AROUND_MICROS;
  const origin = Math.floor(from / BAR_MICROS) * BAR_MICROS;

  const scopes: Scope[] = [
    { key: 'service', filters: [{ field: 'service.name', op: '=', value: service }], scope: 'entry' },
    {
      key: 'operation',
      filters: [
        { field: 'service.name', op: '=', value: service },
        { field: 'name', op: '=', value: operation },
      ],
      scope: 'all',
    },
  ];

  const redHref = (scope: Scope) => {
    const params = new URLSearchParams({
      tab: 'red',
      q: scope.filters.map((filter) => `${filter.field} = '${quoteTraceValue(filter.value)}'`).join(' AND '),
      from: new Date(from / 1_000).toISOString(),
      to: new Date(to / 1_000).toISOString(),
      time: `${new Date(from / 1_000).toISOString()}..${new Date(to / 1_000).toISOString()}`,
    });
    if (scope.scope === 'all') params.set('scope', 'all');
    return `/traces?${params}`;
  };

  return (
    <div className="space-y-3">
      <p className="px-[6px] font-sans text-xs text-tx-3">{t('view.metrics.window')}</p>
      {scopes.map((scope) => (
        <ScopeFigures
          key={scope.key}
          title={scope.key === 'service' ? service : operation}
          caption={t(`view.metrics.${scope.key}`)}
          query={{ from, to, origin, scope }}
          href={redHref(scope)}
        />
      ))}
    </div>
  );
}

function ScopeFigures({
  title,
  caption,
  query,
  href,
}: {
  title: string;
  caption: string;
  query: { from: number; to: number; origin: number; scope: Scope };
  href: string;
}) {
  const { t } = useTranslation('traces');
  const red = useQuery({
    queryKey: ['web', 'span-red', query.scope.key, JSON.stringify(query.scope.filters), query.scope.scope, query.from, query.to],
    staleTime: 60_000,
    queryFn: ({ signal }) =>
      tracesRed(
        {
          from: query.from,
          to: query.to,
          filters: query.scope.filters,
          scope: query.scope.scope,
          bucketTime: BAR_MICROS,
          originTime: query.origin,
        },
        signal,
      ),
  });

  return (
    <section data-testid={`trace-span-metrics-${query.scope.key}`} className="rounded-md bg-bg-1 px-[10px] py-[8px]">
      <div className="flex min-w-0 items-baseline gap-1.5">
        <span className="shrink-0 font-sans text-xs text-tx-3">{caption}</span>
        <span className="min-w-0 flex-1 truncate font-sans text-xs font-strong text-tx-0">{title}</span>
        <Link to={href} className="shrink-0 font-sans text-xs text-[var(--blue-ink)] hover:underline focus-visible:underline focus-visible:outline-none">
          {t('view.metrics.open_red')}
        </Link>
      </div>
      {red.isLoading ? (
        <p className="mt-2 font-sans text-xs text-tx-3">{t('view.metrics.loading')}</p>
      ) : red.isError || !red.data ? (
        <p className="mt-2 font-sans text-xs text-[var(--red-ink)]">{t('view.metrics.failed')}</p>
      ) : red.data.unavailable ? (
        <p className="mt-2 font-sans text-xs text-tx-3">{t(`explore.red.states.unavailable.${red.data.unavailable}`, { defaultValue: t('view.metrics.failed') })}</p>
      ) : (
        <Figures data={red.data} bars={Math.round((query.to - query.origin) / BAR_MICROS)} origin={query.origin} />
      )}
    </section>
  );
}

function Figures({ data, bars, origin }: { data: TraceRedResponse; bars: number; origin: number }) {
  const { t } = useTranslation('traces');
  const series = (pick: (bucket: TraceRedResponse['buckets'][number]) => number | null) => {
    const values: Array<number | null> = Array.from({ length: bars }, () => null);
    for (const bucket of data.buckets) {
      const index = Math.floor((bucket.startTime - origin) / BAR_MICROS);
      if (index >= 0 && index < bars) values[index] = pick(bucket);
    }
    return values;
  };

  const figures = [
    { key: 'requests', value: String(data.total.requests), values: series((bucket) => bucket.requests), variant: 'bars' as const, tone: 'text-[var(--blue-ink)]' },
    {
      key: 'errors',
      value: data.total.errors === null ? '—' : String(data.total.errors),
      values: series((bucket) => bucket.errors),
      variant: 'bars' as const,
      tone: data.total.errors ? 'text-[var(--red-ink)]' : 'text-tx-3',
    },
    {
      key: 'p95',
      value: data.total.p95Ns === null ? '—' : formatTraceDurationNs(data.total.p95Ns),
      values: series((bucket) => bucket.p95Ns),
      variant: 'line' as const,
      tone: 'text-[var(--purple-ink)]',
    },
  ];

  return (
    <div className="mt-2 grid grid-cols-3 gap-3">
      {figures.map((figure) => (
        <div key={figure.key} className="min-w-0">
          <div className="font-sans text-xs text-tx-3">{t(`view.metrics.${figure.key}`)}</div>
          <div className="font-sans text-sm font-strong tabular-nums text-tx-0">{figure.value}</div>
          <Sparkline values={figure.values} variant={figure.variant} zeroBased={figure.variant === 'bars'} className={cn('mt-1 h-[28px]', figure.tone)} />
        </div>
      ))}
    </div>
  );
}
