import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { PencilLine } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as queryApi from '@/api/query';
import { queryResultToDataFrames } from '@/dashboard-engine/dataframe';
import { applyFieldConfig } from '@/dashboard-engine/fieldConfig';
import type { PanelData } from '@/dashboard-engine/schema';
import { VisualizationRenderer } from '@/dashboard-engine/visualizations';
import { toApiError } from '@/lib/http';
import { CopyField } from '@/shell/CopyField';
import { cn } from '@/shell/lib/cn';
import type { HistogramRange } from '@/shell/query/histogram/model';
import { QueryState } from '@/shell/query/State';
import { useQueryTabState } from '@/shell/queryTabs';
import { useAuthStore } from '@/stores/auth';
import type { TimeWindow } from '@/stores/useTimeStore';
import type { QueryResult } from '@/types/query';

import type { LogFieldDef } from '../fieldQueryModel';
import type { CountedLogQuery } from '../histogramCounts';
import { LogStatsChartSwitch, LogStatsControls } from './Controls';
import { CreateStatsDashboard } from './CreateDashboard';
import {
  buildLogStatsStatement,
  defaultLogStatsConfig,
  logStatsConditions,
  logStatsPanel,
  settleLogStatsConfig,
  STATS_GROUP_NONE,
  STATS_GROUP_TIME,
  statsBucketMicros,
  statsChartsFor,
  type LogStatsConfig,
  type StatsChart,
} from './model';
import {
  isResultOverTime,
  isStatisticsStatement,
  sampleStatisticsStatements,
  statsChartsForResult,
} from './written';

/** As many rows as a statement of statistics is asked for: groups beyond these are not drawn. */
const ROW_LIMIT = 1_000;

interface LogStatsViewProps {
  stream: string;
  fields: readonly LogFieldDef[];
  /**
   * The field query the list ran: the statistics are worked out of everything
   * it matches. `null` when the reader wrote a statement instead.
   */
  counted: CountedLogQuery | null;
  /** The time range the query was run over. */
  range: HistogramRange | null;
  /** A statement the reader wrote, and what it returned: its result is what is drawn. */
  written: { statement: string; result: QueryResult } | null;
  timeWindow: TimeWindow;
  /** Puts a statement in the editor and runs it. */
  onOpenStatement: (statement: string) => void;
}

/** The height of an element as it is laid out, kept as it changes. */
function useHeight(element: HTMLElement | null): number {
  const [height, setHeight] = React.useState(0);

  React.useLayoutEffect(() => {
    if (!element) return undefined;

    const measure = () => setHeight(Math.round(element.getBoundingClientRect().height));
    measure();
    if (typeof ResizeObserver === 'undefined') return undefined;

    const observer = new ResizeObserver(measure);
    observer.observe(element);

    return () => observer.disconnect();
  }, [element]);

  return height;
}

/**
 * How tall a chart is drawn: about half of the room, and never so tall that
 * three bars become three slabs. The rows it is drawn from have the rest.
 */
const CHART_HEIGHT = 'h-[clamp(200px,45%,320px)]';
/** One figure needs less than a chart does. */
const STAT_HEIGHT = 'h-[160px]';

/**
 * The statistics of the logs a query matches: a measure, grouped, drawn as a
 * chart, and saved as a dashboard when it is worth keeping.
 *
 * With a field query, the reader picks what is worked out and the statement is
 * written for them over the conditions the list selects by. With a statement of
 * their own, its result is what is drawn. Either way the chart is the dashboard
 * panel itself, so what is saved is what was on the screen.
 */
export function LogStatsView({ stream, fields, counted, range, written, timeWindow, onOpenStatement }: LogStatsViewProps) {
  const { t } = useTranslation('logs');
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const [chosen, setChosen] = useQueryTabState<LogStatsConfig | null>('logs.stats', null);
  const [stage, setStage] = React.useState<HTMLDivElement | null>(null);
  const [sheet, setSheet] = React.useState<HTMLDivElement | null>(null);
  const height = useHeight(stage);
  const sheetHeight = useHeight(sheet);
  const config = React.useMemo(
    () => settleLogStatsConfig(chosen ?? defaultLogStatsConfig(fields), fields),
    [chosen, fields],
  );
  const built = counted !== null && range !== null;
  const statement = React.useMemo(() => {
    if (!counted || !range) return written?.statement ?? '';

    return buildLogStatsStatement({
      stream: counted.stream,
      conditions: logStatsConditions(counted.statement, counted.globalFilters),
      config,
      bucketMicros: statsBucketMicros(range),
    });
  }, [config, counted, range, written?.statement]);
  const answer = useQuery({
    queryKey: ['log-stats', orgId, counted?.stream, statement, range?.start, range?.end, counted?.ranAt],
    queryFn: () =>
      queryApi.runQuery({
        org_id: orgId,
        language: 'sql',
        statement,
        time_range: range!,
        stream: { name: counted!.stream, stream_type: 'logs' },
        limit: ROW_LIMIT,
      }),
    enabled: built && Boolean(orgId),
    // The chart of the choice before stays while the next is worked out.
    placeholderData: keepPreviousData,
    // An answer stands for the run it was asked for; the next run has its own key.
    staleTime: Infinity,
    retry: false,
  });
  const result = built ? answer.data : written?.result;
  const charts = React.useMemo<StatsChart[]>(
    () => (built ? statsChartsFor(config.groupBy) : result ? statsChartsForResult(result) : ['table']),
    [built, config.groupBy, result],
  );
  // A statement of the reader's own is drawn the first way it can be, until another is picked.
  const [writtenChart, setWrittenChart] = React.useState<StatsChart | null>(null);
  const chart = built ? config.chart : writtenChart && charts.includes(writtenChart) ? writtenChart : charts[0]!;
  const overTime = built ? config.groupBy === STATS_GROUP_TIME : result ? isResultOverTime(result) : false;
  const panel = React.useMemo(
    () => (statement ? logStatsPanel({ title: '', stream, statement, chart, overTime }) : null),
    [chart, overTime, statement, stream],
  );
  // The rows a chart is drawn from, as a table under it: the same panel, drawn as rows.
  const sheetPanel = React.useMemo(
    () => (statement ? logStatsPanel({ title: '', stream, statement, chart: 'table', overTime }) : null),
    [overTime, statement, stream],
  );
  const data = React.useMemo<PanelData>(() => {
    const timeRange = { from: range?.start ?? 0, to: range?.end ?? 0 };

    if (built && answer.isError) {
      return { state: 'error', frames: [], error: { message: toApiError(answer.error).message }, timeRange };
    }

    return {
      state: built && answer.isPending ? 'loading' : built && answer.isPlaceholderData ? 'streaming' : 'done',
      // As a dashboard draws the panel: its fields carry what the panel says of them.
      frames: result && panel
        ? applyFieldConfig(queryResultToDataFrames(result, 'A', 'logs'), panel.fieldConfig, panel.overrides)
        : [],
      timeRange,
    };
  }, [answer.error, answer.isError, answer.isPending, answer.isPlaceholderData, built, panel, range, result]);
  // The same rows as the table panel takes them: without what the chart says of its marks.
  const sheetData = React.useMemo<PanelData>(
    () => ({
      ...data,
      frames:
        result && sheetPanel
          ? applyFieldConfig(queryResultToDataFrames(result, 'A', 'logs'), sheetPanel.fieldConfig, sheetPanel.overrides)
          : [],
    }),
    [data, result, sheetPanel],
  );
  // A chart stands over its rows; rows alone, or one figure, stand by themselves.
  const charted = chart !== 'table';
  const sheeted = chart === 'bar' || chart === 'line';
  const suggested = React.useMemo(() => {
    const measure =
      config.measure === 'count'
        ? t('explore.stats.measures.count')
        : `${t(`explore.stats.measures.${config.measure}`)} · ${config.field}`;
    const group =
      config.groupBy === STATS_GROUP_TIME
        ? t('explore.stats.group_time')
        : config.groupBy === STATS_GROUP_NONE
          ? ''
          : config.groupBy;
    const title = built ? [measure, group].filter(Boolean).join(' · ') : t('explore.stats.dashboard.written_panel');

    return { dashboard: t('explore.stats.dashboard.default_name', { stream }), panel: title };
  }, [built, config.field, config.groupBy, config.measure, stream, t]);

  // A statement that lists rows has nothing to chart: say so, and offer one that works something out.
  if (!built && written && !isStatisticsStatement(written.statement)) {
    return (
      <div data-log-stats="samples" className="flex h-full min-h-0 flex-col gap-3 overflow-auto px-4 py-4">
        <div>
          <h3 className="font-sans text-sm font-strong text-tx-0">{t('explore.stats.samples.title')}</h3>
          <p className="mt-1 font-sans text-xs text-tx-2">{t('explore.stats.samples.description')}</p>
        </div>
        {sampleStatisticsStatements(stream, fields, range).map((sample) => (
          <div key={sample} className="rounded-md bg-[var(--control-surface)] p-3">
            <pre className="overflow-auto whitespace-pre-wrap font-code text-xs leading-5 text-tx-1">{sample}</pre>
            <button
              type="button"
              onClick={() => onOpenStatement(sample)}
              data-log-stats-sample
              className="mt-2 rounded font-sans text-xs font-strong text-[var(--blue-ink)] hover:underline focus-visible:underline"
            >
              {t('explore.stats.samples.use')}
            </button>
          </div>
        ))}
      </div>
    );
  }

  return (
    <div data-log-stats={built ? 'built' : 'written'} className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-2 px-4 pb-2 pt-1">
        {built ? (
          <LogStatsControls fields={fields} config={config} onChange={setChosen} />
        ) : (
          <p className="min-w-0 font-sans text-xs text-tx-2">{t('explore.stats.written')}</p>
        )}
        <div className="ml-auto flex items-center gap-2">
          <LogStatsChartSwitch
            charts={charts}
            value={chart}
            onChange={(next) => (built ? setChosen({ ...config, chart: next }) : setWrittenChart(next))}
          />
          <CreateStatsDashboard
            panel={data.state === 'error' || data.frames.length === 0 ? null : panel}
            suggested={suggested}
            timeWindow={timeWindow}
          />
        </div>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-3 px-4">
        <div
          ref={setStage}
          data-log-stats-chart-stage={chart}
          className={cn(
            'min-h-0 shrink-0 overflow-hidden',
            charted ? (chart === 'stat' ? STAT_HEIGHT : CHART_HEIGHT) : 'flex-1',
          )}
        >
          {panel && height > 0 ? (
            <VisualizationRenderer panel={panel} data={data} height={height} cursorScopeId={null} />
          ) : (
            <QueryState state="loading" loadingLabel={t('explore.table.running_query')} className="h-full" />
          )}
        </div>
        {sheeted && sheetPanel && data.state !== 'error' && (
          <div ref={setSheet} data-log-stats-sheet className="min-h-0 flex-1 overflow-hidden rounded-md">
            {sheetHeight > 0 && (
              <VisualizationRenderer panel={sheetPanel} data={sheetData} height={sheetHeight} cursorScopeId={null} />
            )}
          </div>
        )}
      </div>
      {statement && (
        <div className="shrink-0 px-4 pb-3 pt-2" data-log-stats-statement>
          <CopyField
            value={statement}
            copyLabel={t('explore.stats.copy_statement')}
            actions={
              built ? (
                <button
                  type="button"
                  onClick={() => onOpenStatement(statement)}
                  className="mr-1 inline-flex h-7 shrink-0 items-center gap-1.5 rounded px-2 font-sans text-xs font-strong text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
                >
                  <PencilLine className="h-3 w-3" aria-hidden="true" />
                  {t('explore.stats.edit_statement')}
                </button>
              ) : undefined
            }
          >
            {/* On one line, as a field shows it: the statement in full is what is copied. */}
            {statement.replace(/\s+/g, ' ')}
          </CopyField>
        </div>
      )}
    </div>
  );
}
