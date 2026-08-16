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

import { ConfirmDialog } from '@/admin';
import * as pipelinesApi from '@/api/pipelines';
import * as pipelineRunsApi from '@/api/pipelines/runs';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
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
import { Tabs, TabsContent, TabsList } from '@/shell/ui/tabs';

import { NotFound } from '../NotFound';
import {
  PipelineConfigSection,
  PipelineConfigValue,
  PipelineDetailMetadata,
  PipelineKpiBand,
  PipelineRunState,
  PipelineSection,
  PipelineTabTrigger,
  PIPELINE_TYPE_TONE,
  pipelineFlatTableClassName,
} from './CardlessSurface';
import {
  PipelineGraphView,
  pipelineGraphFromPipeline,
  signalTypeFromPipeline,
} from './PipelineGraph';
import {
  getPipelineRunsTableLabels,
  PipelineRunsTable,
} from './PipelineRunsTable';
import {
  formatMillisDuration,
  formatLookback,
  formatRelativeMicros,
  formatSchedule,
  parsePipelineDetailTab,
  pipelineHealth,
  summarizePipelineRuns,
  type PipelineDetailTab,
} from './presentation';

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
  const activeTab = parsePipelineDetailTab(params.get('tab'));

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
  const {
    lastRun,
    runs24h,
    successRate,
    processedRows,
    averageDuration,
    completedRuns,
  } = summarizePipelineRuns(runs);

  const setActiveTab = (value: string) => {
    const next = value as PipelineDetailTab;
    setParams(next === 'overview' ? {} : { tab: next }, { replace: true });
  };
  const runTableLabels = getPipelineRunsTableLabels(t);

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
        <PipelineDetailMetadata
          health={health}
          healthLabel={t(`overview.health.${health}`)}
          typeTone={PIPELINE_TYPE_TONE[type]}
          typeLabel={t(`filters.${type}`)}
          items={[
            {
              label: t('detail.metadata.schedule'),
              value: formatSchedule(pipeline.cron, i18n.language),
            },
            {
              label: t('detail.metadata.lookback'),
              value: formatLookback(pipeline.lookback_secs, i18n.language),
            },
            {
              label: t('detail.metadata.last_run'),
              value: formatRelativeMicros(lastRun?.started_at_micros, i18n.language),
            },
          ]}
          runState={lastRun
            ? {
                state: lastRun.state,
                label: t(`overview.run_states.${lastRun.state}`, {
                  defaultValue: lastRun.state,
                }),
              }
            : undefined}
        />

        <Tabs
          value={activeTab}
          onValueChange={setActiveTab}
          className="flex h-[calc(100%_-_48px)] min-h-0 flex-col"
        >
          <div className="border-b border-bd-0 bg-bg-0 px-6">
            <TabsList className="h-10 max-w-full justify-start overflow-x-auto rounded-none bg-transparent p-0">
              <PipelineTabTrigger value="overview">
                {t('detail.tabs.overview')}
              </PipelineTabTrigger>
              <PipelineTabTrigger value="topology">
                {t('detail.tabs.topology')}
              </PipelineTabTrigger>
              <PipelineTabTrigger value="runs">
                {t('detail.tabs.runs')}
              </PipelineTabTrigger>
              <PipelineTabTrigger value="configuration">
                {t('detail.tabs.configuration')}
              </PipelineTabTrigger>
            </TabsList>
          </div>

          <TabsContent value="overview" className="m-0 min-h-0 flex-1 overflow-auto px-6 py-5">
            <div className="mx-auto flex w-full max-w-[2200px] flex-col gap-6">
              <PipelineKpiBand
                items={[
                  {
                    label: t('detail.overview.last_run'),
                    value: formatRelativeMicros(
                      lastRun?.started_at_micros,
                      i18n.language,
                    ),
                    note: lastRun
                      ? t(`overview.run_states.${lastRun.state}`, { defaultValue: lastRun.state })
                      : '—',
                  },
                  {
                    label: t('detail.overview.success_rate'),
                    value:
                      successRate == null
                        ? '—'
                        : `${successRate.toFixed(successRate === 100 ? 0 : 1)}%`,
                    note: t('detail.overview.window_24h', { count: runs24h.length }),
                    tone: successRate != null && successRate >= 99 ? 'good' : 'neutral',
                  },
                  {
                    label: t('detail.overview.processed_rows'),
                    value: new Intl.NumberFormat(i18n.language, {
                      notation: 'compact',
                    }).format(processedRows),
                    note: t('detail.overview.window_24h', { count: runs24h.length }),
                  },
                  {
                    label: t('detail.overview.average_duration'),
                    value:
                      averageDuration == null
                        ? '—'
                        : formatMillisDuration(averageDuration),
                    note: t('detail.overview.completed_runs', { count: completedRuns }),
                  },
                ]}
              />

              <PipelineSection
                title={t('detail.graph')}
                showHeaderDivider={false}
                actions={
                  <ChromeButton size="sm" onClick={() => setActiveTab('topology')}>
                    {t('detail.view_full_topology')}
                  </ChromeButton>
                }
              >
                <PipelineGraphView model={graph} className="h-[340px]" />
              </PipelineSection>

              <PipelineSection
                title={t('detail.recent_runs')}
                actions={
                  <ChromeButton size="sm" onClick={() => setActiveTab('runs')}>
                    {t('detail.view_all_runs')}
                  </ChromeButton>
                }
                bodyClassName="pt-0"
              >
                <PipelineRunsTable
                  rows={runs.slice(0, 6)}
                  loading={runsQuery.isLoading}
                  compact
                  locale={i18n.language}
                  labels={runTableLabels}
                  renderState={(state) => (
                    <PipelineRunState
                      state={state}
                      label={t(`overview.run_states.${state}`, { defaultValue: state })}
                    />
                  )}
                />
              </PipelineSection>
            </div>
          </TabsContent>

          <TabsContent value="topology" className="m-0 min-h-0 flex-1 overflow-auto px-6 py-5">
            <div className="mx-auto w-full max-w-[2200px]">
              <PipelineSection
                title={t('detail.graph')}
                showHeaderDivider={false}
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
              >
                <PipelineGraphView model={graph} className="h-[560px]" />
              </PipelineSection>
            </div>
          </TabsContent>

          <TabsContent value="runs" className="m-0 min-h-0 flex-1 overflow-auto px-6 py-5">
            <div className="mx-auto w-full max-w-[2200px] overflow-hidden bg-transparent">
              {runsQuery.isError ? (
                <ProductState
                  variant="error"
                  error={runsQuery.error}
                  compact
                  className={pipelineFlatTableClassName}
                />
              ) : (
                <PipelineRunsTable
                  rows={runs}
                  loading={runsQuery.isLoading}
                  locale={i18n.language}
                  labels={runTableLabels}
                  renderState={(state) => (
                    <PipelineRunState
                      state={state}
                      label={t(`overview.run_states.${state}`, { defaultValue: state })}
                    />
                  )}
                />
              )}
            </div>
          </TabsContent>

          <TabsContent value="configuration" className="m-0 min-h-0 flex-1 overflow-auto px-6 py-5">
            <div className="mx-auto w-full max-w-[2200px]">
              <PipelineSection
                title={t('detail.configuration')}
                actions={
                  <div className="flex rounded-md bg-bg-2 p-0.5">
                    {(['structured', 'json'] as const).map((view) => (
                      <button
                        key={view}
                        type="button"
                        onClick={() => setConfigView(view)}
                        className={`rounded px-3 py-1.5 font-sans text-xs font-strong ${
                          configView === view ? 'bg-bg-4 text-tx-0' : 'text-tx-2 hover:text-tx-0'
                        }`}
                      >
                        {t(`detail.config_views.${view}`)}
                      </button>
                    ))}
                  </div>
                }
              >
                {configView === 'structured' ? (
                  <div className="grid gap-x-10 gap-y-4 md:grid-cols-2">
                    <PipelineConfigSection title={t('graph.sources')}>
                      {graph.sources.map((source) => (
                        <PipelineConfigValue key={source}>{source}</PipelineConfigValue>
                      ))}
                    </PipelineConfigSection>
                    <PipelineConfigSection title={t('graph.sinks')}>
                      {graph.sinks.map((sink) => (
                        <PipelineConfigValue key={sink}>{sink}</PipelineConfigValue>
                      ))}
                    </PipelineConfigSection>
                    <PipelineConfigSection title={t('graph.transform')}>
                      {graph.transforms.map((transform, index) => (
                        <PipelineConfigValue key={`${transform.name}-${index}`}>
                          {transform.name} · VRL
                        </PipelineConfigValue>
                      ))}
                    </PipelineConfigSection>
                    <PipelineConfigSection title={t('graph.retry_policy')}>
                      <PipelineConfigValue>
                        {t(`drawer.retry_options.${graph.retryPolicy}`, {
                          defaultValue: graph.retryPolicy,
                        })}
                      </PipelineConfigValue>
                    </PipelineConfigSection>
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
              </PipelineSection>
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

}
