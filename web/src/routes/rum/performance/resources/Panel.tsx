import { X } from 'lucide-react';
import * as React from 'react';
import { Trans, useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import {
  type Group,
  groupFilters,
  type Narrowing,
  phasesQuery,
  readPhases,
  readSlowest,
  readStatuses,
  type Row,
  slowRequests,
  slowestQuery,
  slowestRowsQuery,
  statusesQuery,
  trendQuery,
  traceShare,
  failureRate,
} from './model';
import { formatDurationMs } from '../../_helpers';
import { chartSeries } from '../../explorer/viz/answer';
import type { RumVizBars } from '../../explorer/viz/spec';
import { formatCount } from '../../overview/format';
import { clause, explorerPath, type OverviewRange, type OverviewScope } from '../../overview/model';
import { statusInk, typeInk } from '../../waterfall/ink';
import { PhaseBar, longestPhase } from '../../waterfall/Phases';
import { useDrawerAggregate, useViewRows } from '../drawer/reads';
import { PANEL_CLASS, Section, ShareBar } from '../PanelSection';
import { FIGURE } from '../Reading';
import { failureInk, formatRate } from '../tableParts';

const MS = { divisor: 1_000_000, unit: 'ms' } as const;

/** What the status codes of a group are drawn in: the answers that went wrong in the colour of who is to blame. */
const statusFill = (status: number): string => (status === 0 || status >= 500 ? 'var(--red)' : status >= 400 ? 'var(--orange)' : 'var(--blue)');

/** The query the explorer is opened on for the requests of a group: its values, and what narrows the table. */
function explorerClauses(group: Group, row: Row, narrowing: Narrowing): string[] {
  const status = {
    '': [],
    '4xx': ['resource.status_code >= 400', 'resource.status_code < 500'],
    '5xx': ['resource.status_code >= 500'],
    none: ['resource.status_code = 0'],
  }[narrowing.status];

  return [
    ...group.fields.flatMap((field, index) => {
      const value = row.values[index];

      return value === null || value === undefined ? [] : [clause(field, value)];
    }),
    ...(narrowing.type ? [clause('resource.type', narrowing.type)] : []),
    ...status,
  ];
}

/**
 * What the requests of a group are made of: how they went over the time in view, the parts the
 * requests around the 75th percentile took their time on, how they were answered, and the slowest of
 * them, each of which opens the load of the view that made it.
 */
export function ResourcePanel({
  row,
  group,
  scope,
  range,
  bars,
  narrowing,
  language,
  onClose,
  onOpenView,
}: {
  row: Row;
  group: Group;
  scope: OverviewScope;
  range: OverviewRange;
  bars: RumVizBars;
  narrowing: Narrowing;
  language: string;
  onClose: () => void;
  /** Opens the view that made a request, on that request. */
  onOpenView: (view: string, at: number | null, request: string) => void;
}) {
  const { t } = useTranslation('rum');
  const filters = React.useMemo(() => groupFilters(group, row.values), [group, row.values]);

  const phases = useDrawerAggregate('resources-phases', row.rawP75 === null ? null : phasesQuery(scope, range, narrowing, filters, row.rawP75));
  const statuses = useDrawerAggregate('resources-statuses', statusesQuery(scope, range, narrowing, filters));
  const slowestRead = useDrawerAggregate('resources-slowest', slowestQuery(scope, range, narrowing, filters));
  const trend = useDrawerAggregate('resources-trend', trendQuery(scope, bars, narrowing, filters));

  const slowest = React.useMemo(() => readSlowest(slowestRead.data), [slowestRead.data]);
  const slowRows = useViewRows(slowest.length > 0 ? slowestRowsQuery(scope, range, slowest.map((entry) => entry.id)) : null);
  const slow = React.useMemo(() => slowRequests(slowest, slowRows.data ?? []), [slowRows.data, slowest]);

  const makeup = React.useMemo(() => readPhases(phases.data), [phases.data]);
  const longest = makeup ? longestPhase(makeup.phases) : null;
  const longestMs = makeup && longest ? makeup.phases[longest] : null;
  const codes = React.useMemo(() => readStatuses(statuses.data), [statuses.data]);
  const rate = failureRate(row);
  const traced = traceShare(row);

  const series = React.useMemo(
    () =>
      trend.data
        ? chartSeries(trend.data, {
            bars,
            measures: [{ fn: 'p75', field: 'resource.duration' }],
            groups: [],
            language,
            none: '',
            measureLabel: () => t('performance.resources.panel.trend_series'),
            rescale: MS,
          })
        : [],
    [bars, language, t, trend.data],
  );

  const none = t('performance.resources.table.none');
  const words = row.values.map((value) => value ?? none);
  // A path stands on its host as an address does; the other groups are told apart by their values.
  const title = group.key === 'host_type' ? (words[0] as string) : words.join(group.key === 'path' ? '' : ' · ');
  const type = group.key === 'host_type' ? (row.values[1] ?? null) : null;

  return (
    <aside data-rum-panel="resources" aria-label={t('performance.resources.panel.aria')} className={PANEL_CLASS}>
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-strong text-tx-0" title={title}>
            {title}
          </div>
          <div className="truncate text-xs text-tx-3">
            {type !== null && <span className={cn('font-strong', typeInk(type))}>{type}</span>}
            {type !== null && ' · '}
            {t('performance.resources.panel.requests', { count: row.requests, formatted: formatCount(row.requests, language) })}
          </div>
        </div>
        <button
          type="button"
          aria-label={t('performance.resources.panel.close')}
          onClick={onClose}
          className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md border-0 bg-transparent p-0 text-tx-2 outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
        >
          <X aria-hidden className="h-3.5 w-3.5" />
        </button>
      </div>

      <p data-rum-panel-sentence="" className="m-0 text-sm leading-[21px] text-tx-1">
        {row.p75 !== null && (
          <Trans
            i18nKey={rate === null ? 'performance.resources.panel.sentence_time' : 'performance.resources.panel.sentence'}
            ns="rum"
            values={{ duration: formatDurationMs(row.p75), rate: rate === null ? '' : formatRate(rate) }}
            components={{ n: FIGURE, rate: <span className={cn('font-strong', failureInk(rate))} /> }}
          />
        )}
        {longest && longestMs !== null && (
          <>
            {' '}
            <Trans
              i18nKey="performance.resources.panel.longest"
              ns="rum"
              values={{ part: t(`waterfall.phases.${longest}`), value: formatDurationMs(longestMs) }}
              components={{ n: FIGURE }}
            />
          </>
        )}
      </p>

      <Section
        title={t('performance.resources.panel.trend')}
        pending={trend.isPending}
        failed={trend.isError}
        unavailable={trend.data?.unavailable}
        onRetry={() => void trend.refetch()}
        empty={!series.some((line) => line.data.some((value) => value !== null))}
        emptyText={t('performance.resources.panel.trend_empty')}
        testId="trend"
      >
        <TimeSeriesChart
          height={120}
          series={series}
          xDomain={[bars.frame.start, bars.frame.end]}
          ariaLabel={t('performance.resources.panel.trend_aria')}
          cursorScopeId="rum-resources"
          showLegend={false}
          options={{ drawStyle: 'line', showPoints: 'auto', legendMode: 'hidden', tooltipMode: 'all', leftAxis: { scale: 'linear', softMin: 0, unit: MS.unit } }}
        />
      </Section>

      <Section
        title={t('performance.resources.panel.parts', { formatted: makeup?.requests.toLocaleString(language) ?? '' })}
        pending={phases.isPending && row.rawP75 !== null}
        failed={phases.isError}
        unavailable={phases.data?.unavailable}
        onRetry={() => void phases.refetch()}
        empty={makeup === null}
        emptyText={t('performance.resources.panel.parts_empty')}
        testId="parts"
      >
        {makeup && <PhaseBar phases={makeup.phases} />}
      </Section>

      <Section
        title={t('performance.resources.panel.statuses')}
        pending={statuses.isPending}
        failed={statuses.isError}
        unavailable={statuses.data?.unavailable}
        onRetry={() => void statuses.refetch()}
        empty={codes.length === 0}
        emptyText={t('performance.resources.panel.statuses_empty')}
        testId="statuses"
      >
        <div data-rum-statuses="" className="flex flex-col gap-1.5">
          {codes.map((entry) => (
            <div key={entry.status} data-rum-status-code={entry.status}>
              <ShareBar
                label={entry.status}
                mono
                ink={statusInk(entry.status)}
                labelWidth="34px"
                share={row.withStatus > 0 ? entry.requests / row.withStatus : 0}
                value={row.withStatus > 0 ? formatRate(entry.requests / row.withStatus) : '—'}
                fill={statusFill(entry.status)}
              />
            </div>
          ))}
        </div>
      </Section>

      <Section
        title={t('performance.resources.panel.slowest')}
        pending={slowestRead.isPending || (slowest.length > 0 && slowRows.isPending)}
        failed={slowestRead.isError || slowRows.isError}
        unavailable={slowestRead.data?.unavailable}
        onRetry={() => void (slowestRead.isError ? slowestRead.refetch() : slowRows.refetch())}
        empty={slow.length === 0}
        emptyText={t('performance.resources.panel.slowest_empty')}
        testId="slowest"
      >
        <ol data-rum-slowest="" className="m-0 flex list-none flex-col p-0">
          {slow.map((request) => {
            const line = (
              <>
                <span className="min-w-0 flex-1 truncate font-mono text-tx-0">{[request.method && request.method !== 'GET' ? request.method : null, request.path].filter(Boolean).join(' ')}</span>
                <span className={cn('tabular-nums', request.ms >= 1_000 ? 'text-[var(--red-ink)]' : 'text-[var(--orange-ink)]')}>{formatDurationMs(request.ms)}</span>
                <span className={cn('font-mono', statusInk(request.status))}>{request.status ?? ''}</span>
              </>
            );

            return (
              <li key={request.id} className="border-t border-bd-0 first:border-t-0">
                {request.view ? (
                  <button
                    type="button"
                    data-rum-slow-request={request.id}
                    aria-label={t('performance.resources.panel.open_load', { path: request.path })}
                    onClick={() => onOpenView(request.view as string, request.at, request.id)}
                    className="flex w-full items-center gap-2 border-0 bg-transparent px-0 py-1.5 text-left text-xs outline-none hover:bg-bg-2 focus-visible:bg-bg-3"
                  >
                    {line}
                  </button>
                ) : (
                  <div className="flex items-center gap-2 py-1.5 text-xs">{line}</div>
                )}
              </li>
            );
          })}
        </ol>
      </Section>

      {traced !== null && (
        <div className="flex items-center justify-between gap-2 border-t border-bd-0 pt-2.5 text-xs">
          <span className="text-tx-3">{t('performance.resources.panel.traced', { share: Math.round(traced * 100) })}</span>
          <Link
            to={explorerPath('resources', scope, explorerClauses(group, row, narrowing))}
            className="shrink-0 whitespace-nowrap font-strong text-[var(--blue-ink)] outline-none hover:underline focus-visible:underline"
          >
            {t('performance.resources.panel.explore')}
          </Link>
        </div>
      )}
    </aside>
  );
}
