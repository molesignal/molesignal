import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ChevronDown,
  CirclePause,
  CirclePlay,
  History,
  Pencil,
  RotateCcw,
  Trash2,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { ConfirmDialog, DataTable } from '@/admin';
import * as pipelinesApi from '@/api/pipelines';
import * as pipelineRunsApi from '@/api/pipelines/runs';
import type { PipelineRun } from '@/api/pipelines/runs';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import {
  Card,
  CardBody,
  CardHeader,
  ChromeButton,
  Dot,
  Pill,
  type PillTone,
} from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shell/ui/tabs';

import { NotFound } from '../NotFound';
import {
  PipelineGraphView,
  pipelineGraphFromPipeline,
  signalTypeFromPipeline,
} from './PipelineGraph';
import {
  formatLookback,
  formatRelativeMicros,
  formatRunDuration,
  formatSchedule,
  pipelineHealth,
  type PipelineHealth,
} from './presentation';

const TYPE_TONE = {
  logs: 'orange',
  metrics: 'blue',
  traces: 'green',
} as const;

const HEALTH_TONE: Record<PipelineHealth, PillTone> = {
  healthy: 'green',
  running: 'blue',
  error: 'red',
  paused: 'dim',
  unknown: 'yellow',
  never: 'yellow',
};

type DetailTab = 'overview' | 'topology' | 'runs' | 'configuration';

export function PipelineDetail() {
  const { t, i18n } = useTranslation('pipelines');
  const { id = '' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const pauseAccess = useActionAccess({
    permission: 'pipelines.pause',
  });
  const editAccess = useActionAccess({
    permission: 'pipelines.edit',
  });
  const runAccess = useActionAccess({
    permission: 'pipelines.run',
  });
  const deleteAccess = useActionAccess({
    permission: 'pipelines.delete',
  });
  const [params, setParams] = useSearchParams();
  const [confirmDelete, setConfirmDelete] = React.useState(false);
  const [configView, setConfigView] = React.useState<'structured' | 'json'>('structured');
  const isRemovedLegacyPath = id === 'add';

  const pipelineQuery = useQuery({
    queryKey: ['pipelines', 'get', id],
    queryFn: () => pipelinesApi.get(id),
    enabled: id.length > 0 && !isRemovedLegacyPath,
  });
  const runsQuery = useQuery({
    queryKey: ['pipeline-runs', id, 500],
    queryFn: () => pipelineRunsApi.list(id, undefined, 500),
    enabled: id.length > 0 && !isRemovedLegacyPath,
    refetchInterval: 15_000,
  });
  const pipeline = pipelineQuery.data;
  const runs = runsQuery.data ?? [];
  const tabParam = params.get('tab');
  const activeTab: DetailTab =
    tabParam === 'topology' || tabParam === 'runs' || tabParam === 'configuration'
      ? tabParam
      : 'overview';

  const toggleEnabled = useMutation({
    mutationFn: (enabled: boolean) => {
      if (!pipeline) throw new Error('pipeline not loaded');
      return pipelinesApi.update(id, {
        name: pipeline.name,
        source_stream: pipeline.source_stream ?? '',
        target_stream: pipeline.target_stream ?? '',
        function_steps: pipeline.function_steps ?? {},
        cron: pipeline.cron ?? 'every:5m',
        lookback_secs: pipeline.lookback_secs ?? 300,
        enabled,
      });
    },
    onSuccess: (updated) => {
      qc.setQueryData(['pipelines', 'get', id], updated);
      void qc.invalidateQueries({ queryKey: ['pipelines', 'list'] });
      toast.success(updated.enabled ? t('detail.toast_resumed') : t('detail.toast_paused'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const remove = useMutation({
    mutationFn: () => pipelinesApi.remove(id),
    onSuccess: () => {
      toast.success(t('workspace.toast_deleted'));
      void qc.invalidateQueries({ queryKey: ['pipelines', 'list'] });
      navigate('/pipelines');
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (isRemovedLegacyPath) {
    return <NotFound />;
  }
  if (pipelineQuery.isLoading) {
    return <ProductState variant="loading" />;
  }
  if (pipelineQuery.isError || !pipeline) {
    return <ProductState variant="error" error={pipelineQuery.error} />;
  }

  const type = signalTypeFromPipeline(pipeline);
  const health = pipelineHealth({
    ...pipeline,
    ...(runs[0]?.state ? { last_run_state: runs[0].state } : {}),
  });
  const graph = pipelineGraphFromPipeline(pipeline, type);
  const lastRun = runs[0] ?? null;
  const since24h = Date.now() * 1000 - 24 * 3600 * 1_000_000;
  const runs24h = runs.filter((run) => run.started_at_micros >= since24h);
  const succeeded24h = runs24h.filter((run) => run.state === 'succeeded').length;
  const successRate = runs24h.length > 0 ? (succeeded24h / runs24h.length) * 100 : null;
  const processedRows = runs24h.reduce((total, run) => total + run.scanned_rows, 0);
  const completedDurations = runs24h
    .filter((run) => run.finished_at_micros != null)
    .map((run) => (run.finished_at_micros! - run.started_at_micros) / 1000);
  const averageDuration =
    completedDurations.length > 0
      ? completedDurations.reduce((total, duration) => total + duration, 0) /
        completedDurations.length
      : null;

  const setActiveTab = (value: string) => {
    const next = value as DetailTab;
    setParams(next === 'overview' ? {} : { tab: next }, { replace: true });
  };

  return (
    <>
      <PageHeader
        title={pipeline.name}
        subtitle={t('detail.subtitle')}
        backTo="/pipelines"
        toolbar={
          <>
            <ChromeButton
              disabled={pauseAccess.disabled || toggleEnabled.isPending}
              disabledReason={pauseAccess.reason}
              onClick={() =>
                pauseAccess.allowed &&
                toggleEnabled.mutate(!(pipeline.enabled ?? true))
              }
            >
              {pipeline.enabled ?? true
                ? <CirclePause className="h-3.5 w-3.5" />
                : <CirclePlay className="h-3.5 w-3.5" />}
              {pipeline.enabled ?? true ? t('actions.pause') : t('actions.resume')}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              disabled={editAccess.disabled}
              disabledReason={editAccess.reason}
              onClick={() =>
                editAccess.allowed &&
                navigate(`/pipelines/${encodeURIComponent(id)}/edit`)
              }
            >
              <Pencil className="h-3.5 w-3.5" />
              {t('actions.edit')}
            </ChromeButton>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <ChromeButton>
                  {t('workspace.more_actions')}
                  <ChevronDown className="h-3.5 w-3.5" />
                </ChromeButton>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-44">
                <DropdownMenuItem
                  onSelect={() => navigate(`/pipelines/${encodeURIComponent(id)}/history`)}
                >
                  <History className="h-3.5 w-3.5" />
                  {t('flows.edit.history')}
                </DropdownMenuItem>
                <DropdownMenuItem
                  disabled={runAccess.disabled}
                  disabledReason={runAccess.reason}
                  onSelect={() =>
                    runAccess.allowed &&
                    navigate(`/pipelines/${encodeURIComponent(id)}/backfill`)
                  }
                >
                  <RotateCcw className="h-3.5 w-3.5" />
                  {t('flows.edit.backfill')}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  disabled={deleteAccess.disabled}
                  disabledReason={deleteAccess.reason}
                  className="text-red-soft focus:text-red-soft"
                  onSelect={() =>
                    deleteAccess.allowed && setConfirmDelete(true)
                  }
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  {t('workspace.delete')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
      />
      <PageBody
        padded={false}
        className="!min-h-0 h-[calc(100vh-var(--topbar-h)-var(--pageheader-h,0px))] overflow-hidden"
      >
        <div className="flex min-h-12 flex-wrap items-center gap-x-6 gap-y-2 border-b border-bd-0 bg-bg-1 px-6 py-2.5 font-sans text-xs">
          <Pill tone={HEALTH_TONE[health]}>
            <Dot
              tone={
                health === 'error'
                  ? 'red'
                  : health === 'paused'
                    ? 'dim'
                    : health === 'running'
                      ? 'blue'
                      : health === 'healthy'
                        ? 'green'
                        : 'yellow'
              }
            />
            {t(`overview.health.${health}`)}
          </Pill>
          <Pill tone={TYPE_TONE[type]}>{t(`filters.${type}`)}</Pill>
          <Metadata label={t('detail.metadata.schedule')} value={formatSchedule(pipeline.cron, i18n.language)} />
          <Metadata
            label={t('detail.metadata.lookback')}
            value={formatLookback(pipeline.lookback_secs, i18n.language)}
          />
          <Metadata
            label={t('detail.metadata.last_run')}
            value={formatRelativeMicros(lastRun?.started_at_micros, i18n.language)}
          />
          {lastRun && <RunState state={lastRun.state} />}
        </div>

        <Tabs
          value={activeTab}
          onValueChange={setActiveTab}
          className="flex h-[calc(100%_-_48px)] min-h-0 flex-col"
        >
          <div className="border-b border-bd-0 bg-bg-1 px-6 py-2">
            <TabsList className="bg-bg-2">
              <TabsTrigger value="overview">{t('detail.tabs.overview')}</TabsTrigger>
              <TabsTrigger value="topology">{t('detail.tabs.topology')}</TabsTrigger>
              <TabsTrigger value="runs">{t('detail.tabs.runs')}</TabsTrigger>
              <TabsTrigger value="configuration">{t('detail.tabs.configuration')}</TabsTrigger>
            </TabsList>
          </div>

          <TabsContent value="overview" className="m-0 min-h-0 flex-1 overflow-auto p-5">
            <div className="mx-auto flex max-w-[1500px] flex-col gap-4">
              <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
                <OverviewStat
                  label={t('detail.overview.last_run')}
                  value={formatRelativeMicros(lastRun?.started_at_micros, i18n.language)}
                  sub={lastRun ? t(`overview.run_states.${lastRun.state}`, { defaultValue: lastRun.state }) : '—'}
                />
                <OverviewStat
                  label={t('detail.overview.success_rate')}
                  value={successRate == null ? '—' : `${successRate.toFixed(successRate === 100 ? 0 : 1)}%`}
                  sub={t('detail.overview.window_24h', { count: runs24h.length })}
                />
                <OverviewStat
                  label={t('detail.overview.processed_rows')}
                  value={new Intl.NumberFormat(i18n.language, { notation: 'compact' }).format(processedRows)}
                  sub={t('detail.overview.window_24h', { count: runs24h.length })}
                />
                <OverviewStat
                  label={t('detail.overview.average_duration')}
                  value={averageDuration == null ? '—' : formatMillisDuration(averageDuration)}
                  sub={t('detail.overview.completed_runs', { count: completedDurations.length })}
                />
              </div>

              <Card>
                <CardHeader
                  title={t('detail.graph')}
                  actions={
                    <ChromeButton size="sm" onClick={() => setActiveTab('topology')}>
                      {t('detail.view_full_topology')}
                    </ChromeButton>
                  }
                />
                <CardBody>
                  <PipelineGraphView model={graph} className="h-[340px]" />
                </CardBody>
              </Card>

              <Card>
                <CardHeader
                  title={t('detail.recent_runs')}
                  actions={
                    <ChromeButton size="sm" onClick={() => setActiveTab('runs')}>
                      {t('detail.view_all_runs')}
                    </ChromeButton>
                  }
                />
                <div className="border-t border-bd-0">
                  <RunsTable rows={runs.slice(0, 6)} compact />
                </div>
              </Card>
            </div>
          </TabsContent>

          <TabsContent value="topology" className="m-0 min-h-0 flex-1 overflow-auto p-5">
            <div className="mx-auto max-w-[1600px]">
              <Card>
                <CardHeader
                  title={t('detail.graph')}
                  actions={
                    <span className="font-sans text-xs text-tx-3">
                      {t('workspace.graph_stats', {
                        nodes: graph.sources.length + graph.transforms.length + graph.sinks.length,
                        edges:
                          graph.sources.length +
                          Math.max(0, graph.transforms.length - 1) +
                          graph.sinks.length,
                      })}
                    </span>
                  }
                />
                <CardBody>
                  <PipelineGraphView model={graph} className="h-[560px]" />
                </CardBody>
              </Card>
            </div>
          </TabsContent>

          <TabsContent value="runs" className="m-0 min-h-0 flex-1 overflow-auto p-5">
            <div className="mx-auto max-w-[1600px] overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
              {runsQuery.isError ? (
                <ProductState variant="error" error={runsQuery.error} compact />
              ) : (
                <RunsTable rows={runs} />
              )}
            </div>
          </TabsContent>

          <TabsContent value="configuration" className="m-0 min-h-0 flex-1 overflow-auto p-5">
            <div className="mx-auto max-w-5xl">
              <div className="mb-3 flex items-center justify-between">
                <h2 className="m-0 font-sans text-base font-strong text-tx-0">
                  {t('detail.configuration')}
                </h2>
                <div className="flex rounded-md border border-bd-0 bg-bg-2 p-0.5">
                  {(['structured', 'json'] as const).map((view) => (
                    <button
                      key={view}
                      type="button"
                      onClick={() => setConfigView(view)}
                      className={`rounded px-3 py-1.5 font-sans text-xs font-strong ${
                        configView === view ? 'bg-bg-1 text-tx-0 shadow-sm' : 'text-tx-2'
                      }`}
                    >
                      {t(`detail.config_views.${view}`)}
                    </button>
                  ))}
                </div>
              </div>
              {configView === 'structured' ? (
                <div className="grid gap-3 md:grid-cols-2">
                  <ConfigSection title={t('graph.sources')}>
                    {graph.sources.map((source) => <ConfigValue key={source}>{source}</ConfigValue>)}
                  </ConfigSection>
                  <ConfigSection title={t('graph.sinks')}>
                    {graph.sinks.map((sink) => <ConfigValue key={sink}>{sink}</ConfigValue>)}
                  </ConfigSection>
                  <ConfigSection title={t('graph.transform')}>
                    {graph.transforms.map((transform, index) => (
                      <ConfigValue key={`${transform.name}-${index}`}>
                        {transform.name} · VRL
                      </ConfigValue>
                    ))}
                  </ConfigSection>
                  <ConfigSection title={t('graph.retry_policy')}>
                    <ConfigValue>
                      {t(`drawer.retry_options.${graph.retryPolicy}`, {
                        defaultValue: graph.retryPolicy,
                      })}
                    </ConfigValue>
                  </ConfigSection>
                </div>
              ) : (
                <CodeEditor
                  value={JSON.stringify(pipeline, null, 2)}
                  language="json"
                  label="JSON"
                  ariaLabel={t('detail.configuration')}
                  readOnly
                  minHeight={520}
                  maxHeight={720}
                />
              )}
            </div>
          </TabsContent>
        </Tabs>
      </PageBody>
      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        destructive
        title={t('workspace.delete_confirm_title')}
        description={t('workspace.delete_confirm_description')}
        confirmLabel={t('workspace.delete_confirm_label')}
        busy={remove.isPending}
        disabled={deleteAccess.disabled}
        disabledReason={deleteAccess.reason}
        onConfirm={() => deleteAccess.allowed && remove.mutate()}
      />
    </>
  );

  function RunState({ state }: { state: string }) {
    const tone =
      state === 'succeeded' ? 'green' : state === 'failed' ? 'red' : state === 'running' ? 'blue' : 'dim';
    return (
      <span className="inline-flex items-center gap-1.5 text-tx-2">
        <Dot tone={tone} />
        {t(`overview.run_states.${state}`, { defaultValue: state })}
      </span>
    );
  }

  function RunsTable({ rows, compact = false }: { rows: PipelineRun[]; compact?: boolean }) {
    if (runsQuery.isLoading) return <ProductState variant="loading" compact />;
    if (rows.length === 0) {
      return (
        <ProductState
          variant="empty"
          compact
          title={t('detail.runs_empty_title')}
          description={t('detail.runs_empty_description')}
        />
      );
    }
    return (
      <DataTable
        rows={rows}
        rowKey={(run) => run.id}
        columns={[
          {
            key: 'state',
            header: t('detail.run_columns.state'),
            width: 120,
            cell: (run) => <RunState state={run.state} />,
          },
          {
            key: 'started',
            header: t('detail.run_columns.started'),
            width: 200,
            cell: (run) => (
              <div>
                <div className="text-tx-1">
                  {formatRelativeMicros(run.started_at_micros, i18n.language)}
                </div>
                {!compact && (
                  <div className="mt-0.5 font-mono text-xs text-tx-3">
                    {new Date(run.started_at_micros / 1000).toLocaleString(i18n.language)}
                  </div>
                )}
              </div>
            ),
          },
          {
            key: 'duration',
            header: t('flows.history.columns.duration'),
            width: 130,
            cell: (run) => formatRunDuration(run.started_at_micros, run.finished_at_micros),
          },
          {
            key: 'rows',
            header: t('detail.run_columns.rows'),
            width: 140,
            cell: (run) => new Intl.NumberFormat(i18n.language).format(run.scanned_rows),
          },
          {
            key: 'error',
            header: t('detail.run_columns.error'),
            cell: (run) => (
              <span className={run.error ? 'text-red-soft' : 'text-tx-3'}>{run.error ?? '—'}</span>
            ),
          },
        ]}
      />
    );
  }
}

function Metadata({ label, value }: { label: React.ReactNode; value: React.ReactNode }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className="text-tx-3">{label}</span>
      <span className="font-strong text-tx-1">{value}</span>
    </span>
  );
}

function OverviewStat({
  label,
  value,
  sub,
}: {
  label: React.ReactNode;
  value: React.ReactNode;
  sub: React.ReactNode;
}) {
  return (
    <div className="rounded-lg border border-bd-0 bg-bg-1 px-4 py-3.5">
      <div className="font-sans text-xs font-strong text-tx-2">{label}</div>
      <div className="mt-2 font-sans text-2xl font-display-strong tabular-nums text-tx-0">{value}</div>
      <div className="mt-1 font-sans text-xs text-tx-3">{sub}</div>
    </div>
  );
}

function ConfigSection({
  title,
  children,
}: {
  title: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-lg border border-bd-0 bg-bg-1 p-4">
      <h3 className="m-0 font-sans text-xs font-strong text-tx-2">{title}</h3>
      <div className="mt-3 flex flex-col gap-2">{children}</div>
    </section>
  );
}

function ConfigValue({ children }: { children: React.ReactNode }) {
  return (
    <div className="rounded-md border border-bd-0 bg-bg-2 px-3 py-2 font-mono text-xs text-tx-1">
      {children}
    </div>
  );
}

function formatMillisDuration(millis: number): string {
  if (millis < 1000) return `${millis.toFixed(0)} ms`;
  if (millis < 60_000) return `${(millis / 1000).toFixed(1)} s`;
  return `${(millis / 60_000).toFixed(1)} min`;
}
