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

import { DataTable } from '@/admin';
import * as pipelinesApi from '@/api/pipelines';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import {
  ChromeButton,
  Dot,
  Pill,
  QueryInput,
  type PillTone,
} from '@/shell/chrome';
import { FormSelect } from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';

import {
  signalTypeFromPipeline,
  type PipelineSignalType,
} from './PipelineGraph';
import {
  formatRelativeMicros,
  formatSchedule,
  pipelineHealth,
  pipelineSuccessRate,
  type PipelineHealth,
} from './presentation';

interface DisplayPipeline extends pipelinesApi.ScheduledPipeline {
  type: PipelineSignalType;
  health: PipelineHealth;
}

type DotTone = NonNullable<React.ComponentProps<typeof Dot>['tone']>;

const TYPE_TONE: Record<PipelineSignalType, PillTone> = {
  logs: 'orange',
  metrics: 'blue',
  traces: 'green',
};

const HEALTH_TONE: Record<PipelineHealth, { pill: PillTone; dot: DotTone }> = {
  healthy: { pill: 'green', dot: 'green' },
  running: { pill: 'blue', dot: 'blue' },
  error: { pill: 'red', dot: 'red' },
  paused: { pill: 'dim', dot: 'dim' },
  unknown: { pill: 'yellow', dot: 'yellow' },
  never: { pill: 'yellow', dot: 'yellow' },
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
        pipeline.source_stream?.toLowerCase().includes(needle) ||
        pipeline.target_stream?.toLowerCase().includes(needle);
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
    (total, pipeline) => total + (pipeline.failed_runs_24h ?? 0),
    0,
  );

  return (
    <ListPage
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
            {(['all', 'logs', 'metrics', 'traces'] as const).map((kind) => {
              const count = kind === 'all'
                ? pipelines.length
                : pipelines.filter((pipeline) => pipeline.type === kind).length;
              return (
                <button
                  key={kind}
                  type="button"
                  onClick={() => setTypeFilter(kind)}
                  className={`rounded px-2.5 py-1.5 font-sans text-xs font-strong ${
                    typeFilter === kind ? 'bg-bg-4 text-tx-0' : 'text-tx-2 hover:text-tx-0'
                  }`}
                >
                  {t(`filters.${kind === 'all' ? 'all_types' : kind}`)}
                  <span className="ml-1.5 font-mono text-tx-3">{count}</span>
                </button>
              );
            })}
          </div>
          <FormSelect
            value={healthFilter}
            onChange={(value) => setHealthFilter(value as 'all' | PipelineHealth)}
            options={[
              { value: 'all', label: t('overview.filters.all_statuses') },
              { value: 'healthy', label: t('overview.health.healthy') },
              { value: 'running', label: t('overview.health.running') },
              { value: 'error', label: t('overview.health.error') },
              { value: 'paused', label: t('overview.health.paused') },
              { value: 'unknown', label: t('overview.health.unknown') },
              { value: 'never', label: t('overview.health.never') },
            ]}
            className="w-40 bg-bg-1"
          />
        </div>
      }
      state={listState}
      bodyClassName="space-y-4"
    >
      <div className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
        <DataTable
          rows={filtered}
          rowKey={(pipeline) => pipeline.id}
          onRowClick={(pipeline) => navigate(`/pipelines/${encodeURIComponent(pipeline.id)}`)}
          emptyLabel={t('overview.no_matches')}
          columns={[
            {
              key: 'name',
              header: t('overview.columns.name'),
              width: '30%',
              cell: (pipeline) => (
                <div className="min-w-0 py-1">
                  <div className="truncate font-sans text-sm font-strong text-tx-0">
                    {pipeline.name}
                  </div>
                  <div className="mt-1 truncate font-mono text-xs text-tx-3">
                    {pipeline.source_stream ?? '—'} → {pipeline.target_stream ?? '—'}
                  </div>
                </div>
              ),
            },
            {
              key: 'type',
              header: t('overview.columns.type'),
              width: 110,
              cell: (pipeline) => (
                <Pill tone={TYPE_TONE[pipeline.type]}>{t(`filters.${pipeline.type}`)}</Pill>
              ),
            },
            {
              key: 'schedule',
              header: t('overview.columns.schedule'),
              width: 140,
              cell: (pipeline) => formatSchedule(pipeline.cron, i18n.language),
            },
            {
              key: 'status',
              header: t('overview.columns.status'),
              width: 130,
              cell: (pipeline) => <HealthBadge health={pipeline.health} />,
            },
            {
              key: 'last_run',
              header: t('overview.columns.last_run'),
              width: 210,
              cell: (pipeline) => (
                <div className="flex min-w-0 items-center gap-2">
                  <RunState state={pipeline.last_run_state} />
                  <span className="truncate text-tx-2">
                    {formatRelativeMicros(
                      pipeline.last_run_started_at_micros ?? pipeline.last_run_at_micros,
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
              cell: (pipeline) => {
                const rate = pipelineSuccessRate(pipeline);
                return rate == null ? '—' : `${rate.toFixed(rate === 100 ? 0 : 1)}%`;
              },
            },
            {
              key: 'rows',
              header: t('overview.columns.rows'),
              width: 110,
              cell: (pipeline) =>
                pipeline.last_run_scanned_rows == null
                  ? '—'
                  : new Intl.NumberFormat(i18n.language).format(pipeline.last_run_scanned_rows),
            },
            {
              key: 'open',
              header: '',
              width: 48,
              cell: () => <ChevronRight className="h-4 w-4 text-tx-3" />,
            },
          ]}
        />
      </div>
    </ListPage>
  );

  function HealthBadge({ health }: { health: PipelineHealth }) {
    const tone = HEALTH_TONE[health];
    return (
      <Pill tone={tone.pill}>
        <Dot tone={tone.dot} />
        {t(`overview.health.${health}`)}
      </Pill>
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
