import { useQuery } from '@tanstack/react-query';
import {
  ChevronRight,
  Link2,
  Plus,
  RadioTower,
  RefreshCw,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { DataTable, TableNoMatch } from '@/admin';
import * as pipelinesApi from '@/api/pipelines';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import {
  ChromeButton,
  Dot,
  Pill,
  QueryInput,
} from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { StatusWord, type StatusTone } from '@/shell/StatusWord';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

import { alertRecordLine } from './alertRecord';
import {
  pipelineStreams,
  signalTypeFromPipeline,
  type PipelineSignalType,
} from './PipelineGraph';
import { alertStageOf, executionMode } from './PipelineGraph/model';
import {
  formatRelativeMicros,
  formatSchedule,
  pipelineHealth,
  pipelineSuccessRate,
  type PipelineHealth,
} from './presentation';
import {
  PIPELINE_TYPE_TONE,
  pipelineFlatTableClassName,
} from './Surfaces';

interface DisplayPipeline extends pipelinesApi.ScheduledPipeline {
  type: PipelineSignalType;
  health: PipelineHealth;
  /** Every stream it reads and writes, its own first. */
  streams: { sources: string[]; sinks: string[] };
}

type DotTone = NonNullable<React.ComponentProps<typeof Dot>['tone']>;

/** Sorted by status, what wants attention comes first. */
const HEALTH_ORDER: Record<PipelineHealth, number> = {
  error: 0,
  unknown: 1,
  never: 2,
  paused: 3,
  running: 4,
  listening: 5,
  ready: 6,
  healthy: 7,
};

const HEALTH_TONE: Record<PipelineHealth, StatusTone> = {
  healthy: 'green',
  running: 'blue',
  error: 'red',
  paused: 'dim',
  unknown: 'yellow',
  ready: 'blue',
  listening: 'blue',
  never: 'yellow',
};

export function Pipelines() {
  const { t, i18n } = useTranslation('pipelines');
  const navigate = useNavigate();
  const createAccess = useActionAccess({
    permission: 'pipelines.create',
  });
  const [search, setSearch] = React.useState('');
  const [typeFilter, setTypeFilter] = React.useState<'all' | PipelineSignalType>('all');
  const [healthFilter, setHealthFilter] = React.useState<'all' | PipelineHealth>('all');

  const listQuery = useQuery({
    queryKey: ['pipelines', 'list'],
    queryFn: () => pipelinesApi.list(),
    refetchInterval: 15_000,
    refetchIntervalInBackground: true,
    refetchOnWindowFocus: 'always',
  });

  const pipelines = React.useMemo<DisplayPipeline[]>(
    () =>
      (listQuery.data ?? []).map((pipeline) => ({
        ...pipeline,
        type: signalTypeFromPipeline(pipeline),
        health: pipelineHealth(pipeline),
        streams: pipelineStreams(pipeline),
      })),
    [listQuery.data],
  );
  const filtered = React.useMemo(() => {
    const needle = search.trim().toLowerCase();
    return pipelines.filter((pipeline) => {
      const matchesType = typeFilter === 'all' || pipeline.type === typeFilter;
      const matchesHealth = healthFilter === 'all' || pipeline.health === healthFilter;
      const matchesSearch =
        !needle ||
        pipeline.name.toLowerCase().includes(needle) ||
        [...pipeline.streams.sources, ...pipeline.streams.sinks].some((stream) =>
          stream.toLowerCase().includes(needle),
        );
      return matchesType && matchesHealth && matchesSearch;
    });
  }, [healthFilter, pipelines, search, typeFilter]);

  const state = queryStateFor({
    isLoading: listQuery.isLoading,
    isError: listQuery.isError,
    data: filtered,
  });
  const listState: ProductStateProps | null =
    state === 'loading'
      ? { variant: 'loading' }
      : state === 'error'
        ? { variant: 'error', error: listQuery.error }
        : state === 'empty' && pipelines.length === 0
          ? {
              variant: 'empty',
              title: t('states.empty_title'),
              description: t('states.empty_description'),
              action: (
                <ChromeButton
                  variant="primary"
                  disabled={createAccess.disabled}
                  disabledReason={createAccess.reason}
                  onClick={() =>
                    createAccess.allowed && navigate('/pipelines/new')
                  }
                >
                  <Plus className="h-3.5 w-3.5" /> {t('actions.new_pipeline')}
                </ChromeButton>
              ),
            }
          : null;

  const enabledCount = pipelines.filter((pipeline) => pipeline.enabled !== false).length;
  const errorCount = pipelines.filter((pipeline) => pipeline.health === 'error').length;
  const pausedCount = pipelines.filter((pipeline) => pipeline.health === 'paused').length;
  const failedRuns24h = pipelines.reduce(
    (total, pipeline) => total + (pipeline.failedRuns24h ?? 0),
    0,
  );

  return (
    <ListPage
      appearance="surface"
      title={t('title')}
      subtitle={t('overview.subtitle')}
      toolbar={
        <>
          <ChromeButton onClick={() => navigate('/datasource')}>
            <RadioTower className="h-3.5 w-3.5" />
            {t('actions.sources')}
          </ChromeButton>
          <ChromeButton onClick={() => navigate('/pipelines/connectors')}>
            <Link2 className="h-3.5 w-3.5" />
            {t('actions.connectors')}
          </ChromeButton>
          <ChromeButton
            disabled={listQuery.isFetching}
            onClick={() => void listQuery.refetch()}
            title={t('actions.refresh')}
          >
            <RefreshCw
              className={`h-3.5 w-3.5 ${listQuery.isFetching ? 'animate-spin' : ''}`}
            />
            {t('actions.refresh')}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={createAccess.disabled}
            disabledReason={createAccess.reason}
            onClick={() => createAccess.allowed && navigate('/pipelines/new')}
          >
            <Plus className="h-3.5 w-3.5" /> {t('actions.new_pipeline')}
          </ChromeButton>
        </>
      }
      kpis={[
        {
          label: t('overview.kpis.running'),
          value: String(enabledCount - errorCount),
          sub: t('overview.kpis.running_hint'),
          tone: 'good',
        },
        {
          label: t('overview.kpis.error'),
          value: String(errorCount),
          sub: t('overview.kpis.error_hint'),
          tone: errorCount > 0 ? 'danger' : 'neutral',
        },
        {
          label: t('overview.kpis.paused'),
          value: String(pausedCount),
          sub: t('overview.kpis.paused_hint'),
          tone: pausedCount > 0 ? 'warn' : 'neutral',
        },
        {
          label: t('overview.kpis.failed_24h'),
          value: String(failedRuns24h),
          sub: t('overview.kpis.failed_24h_hint'),
          tone: failedRuns24h > 0 ? 'danger' : 'neutral',
        },
      ]}
      filters={
        <div className="flex w-full flex-wrap items-center gap-3">
          <QueryInput
            value={search}
            onChange={setSearch}
            placeholder={t('filters.search_placeholder') ?? ''}
            className="h-9 min-w-[220px] max-w-[360px] flex-1"
          />
          <div className="flex gap-1 rounded-md border border-bd-0 bg-bg-2 p-0.5">
            {(['all', 'logs', 'metrics', 'traces', 'profiles'] as const).map((kind) => {
              const count = kind === 'all'
                ? pipelines.length
                : pipelines.filter((pipeline) => pipeline.type === kind).length;
              return (
                <button
                  key={kind}
                  type="button"
                  onClick={() => setTypeFilter(kind)}
                  className={`rounded-[5px] px-2.5 py-1 font-sans text-xs font-strong ${
                    typeFilter === kind ? 'bg-bg-4 text-tx-0' : 'text-tx-2 hover:text-tx-0'
                  }`}
                >
                  {t(`filters.${kind === 'all' ? 'all_types' : kind}`)}
                  <span className="ml-1.5 font-mono text-tx-3">{listQuery.isSuccess ? count : '–'}</span>
                </button>
              );
            })}
          </div>
          <Select
            value={healthFilter}
            onValueChange={(value) =>
              setHealthFilter(value as 'all' | PipelineHealth)
            }
          >
            <SelectTrigger
              className="h-9 w-40 bg-[var(--control-surface)] px-3 font-sans text-tx-1"
              aria-label={t('overview.filters.all_statuses')}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent align="start">
              <SelectItem value="all" className="h-8">
                {t('overview.filters.all_statuses')}
              </SelectItem>
              {(['ready', 'listening', 'healthy', 'running', 'error', 'paused', 'unknown', 'never'] as const).map(
                (health) => (
                  <SelectItem key={health} value={health} className="h-8">
                    {t(`overview.health.${health}`)}
                  </SelectItem>
                ),
              )}
            </SelectContent>
          </Select>
        </div>
      }
      state={listState}
    >
      <DataTable
        className={pipelineFlatTableClassName}
        rows={filtered}
        rowKey={(pipeline) => pipeline.id}
        onRowClick={(pipeline) => navigate(`/pipelines/${encodeURIComponent(pipeline.id)}`)}
        emptyLabel={
          <TableNoMatch
            label={t('overview.no_matches')}
            onClear={() => {
              setSearch('');
              setTypeFilter('all');
              setHealthFilter('all');
            }}
          />
        }
        columns={[
            {
              key: 'name',
              header: t('overview.columns.name'),
              width: '30%',
              sortable: true,
              value: (pipeline) => pipeline.name,
              cell: (pipeline) => (
                <div className="min-w-0 py-1">
                  <div className="truncate font-sans text-sm font-strong text-tx-0">
                    {pipeline.name}
                  </div>
                  <div className="mt-1 truncate font-mono text-xs text-tx-3">
                    {executionMode(pipeline) === 'alert'
                      ? t(alertStageOf(pipeline) === 'notify' ? 'alert_mode.list_line_notify' : 'alert_mode.list_line')
                      : `${pipeline.streams.sources.join(', ') || '—'} → ${pipeline.streams.sinks.join(', ') || '—'}`}
                  </div>
                </div>
              ),
            },
            {
              key: 'type',
              header: t('overview.columns.type'),
              width: 110,
              cell: (pipeline) =>
                executionMode(pipeline) === 'alert' ? (
                  <Pill tone="red">{t('alert_mode.signal')}</Pill>
                ) : (
                  <Pill tone={PIPELINE_TYPE_TONE[pipeline.type]}>{t(`filters.${pipeline.type}`)}</Pill>
                ),
            },
            {
              key: 'schedule',
              header: t('overview.columns.schedule'),
              width: 140,
              cell: (pipeline) => {
                const mode = executionMode(pipeline);
                if (mode === 'alert') {
                  return t(alertStageOf(pipeline) === 'notify' ? 'alert_mode.schedule_notify' : 'alert_mode.schedule');
                }
                return mode === 'realtime' ? t('realtime.title') : formatSchedule(pipeline.cron, i18n.language);
              },
            },
            {
              key: 'status',
              header: t('overview.columns.status'),
              width: 130,
              sortable: true,
              sortValue: (pipeline) => HEALTH_ORDER[pipeline.health],
              cell: (pipeline) => <HealthBadge health={pipeline.health} />,
            },
            {
              key: 'last_run',
              header: t('overview.columns.last_run'),
              width: 210,
              sortable: true,
              sortValue: (pipeline) => pipeline.lastRunStartedAt ?? pipeline.lastRunAt ?? null,
              cell: (pipeline) =>
                // An alert pipeline has no runs: what it did today stands in their place.
                executionMode(pipeline) === 'alert' ? (
                  <span className="truncate text-tx-2">{alertRecordLine(pipeline, t)}</span>
                ) : (
                  <div className="flex min-w-0 items-center gap-2">
                    <RunState state={pipeline.lastRunState} />
                    <span className="truncate text-tx-2">
                      {formatRelativeMicros(
                        pipeline.lastRunStartedAt ?? pipeline.lastRunAt,
                        i18n.language,
                      )}
                    </span>
                  </div>
                ),
            },
            {
              key: 'success_rate',
              header: t('overview.columns.success_rate'),
              width: 120,
              sortable: true,
              sortValue: (pipeline) => pipelineSuccessRate(pipeline),
              cell: (pipeline) => {
                const rate = pipelineSuccessRate(pipeline);
                return rate == null ? '—' : `${rate.toFixed(rate === 100 ? 0 : 1)}%`;
              },
            },
            {
              key: 'rows',
              header: t('overview.columns.rows'),
              width: 110,
              sortable: true,
              sortValue: (pipeline) => pipeline.lastRunScannedRows ?? null,
              cell: (pipeline) =>
                pipeline.lastRunScannedRows == null
                  ? '—'
                  : new Intl.NumberFormat(i18n.language).format(pipeline.lastRunScannedRows),
            },
            {
              key: 'open',
              header: '',
              width: 48,
              cell: () => <ChevronRight className="h-4 w-4 text-tx-3" />,
            },
        ]}
      />
    </ListPage>
  );

  function HealthBadge({ health }: { health: PipelineHealth }) {
    return (
      <StatusWord tone={HEALTH_TONE[health]} strong={health === 'error'}>
        {t(`overview.health.${health}`)}
      </StatusWord>
    );
  }

  function RunState({ state: runState }: { state: string | undefined }) {
    if (!runState) return null;
    const tone: DotTone =
      runState === 'succeeded'
        ? 'green'
        : runState === 'failed'
          ? 'red'
          : runState === 'running'
            ? 'blue'
            : 'dim';
    return (
      <span className="inline-flex shrink-0 items-center gap-1.5">
        <Dot tone={tone} />
        <span>{t(`overview.run_states.${runState}`, { defaultValue: runState })}</span>
      </span>
    );
  }
}
