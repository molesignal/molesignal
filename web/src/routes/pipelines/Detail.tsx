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
import { JsonView } from '@/shell/JsonEditor';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { Tabs, TabsContent } from '@/shell/ui/tabs';

import { NotFound } from '../NotFound';
import { todaysAlertRecord } from './alertRecord';
import { auditReaderName } from './auditReader';
import { PipelineLimits } from './counting/LimitUsage';
import { pipelineKpis } from './detailKpis';
import { PipelineFlow } from './editor/Flow';
import { useConnectorNames } from './editor/resources';
import {
  connectorOf,
  executionMode,
  firstStreamIndex,
  isRouted,
  pipelineGraphFromPipeline,
  pipelineGraphStats,
  signalTypeFromPipeline,
  stepType,
  tableOf,
} from './PipelineGraph';
import {
  getPipelineRunsTableLabels,
  PipelineRunsTable,
} from './PipelineRunsTable';
import {
  formatLookback,
  formatRelativeMicros,
  formatSchedule,
  parsePipelineDetailTab,
  pipelineHealth,
  summarizePipelineRuns,
  type PipelineDetailTab,
} from './presentation';
import {
  PipelineConfigSection,
  PipelineConfigValue,
  PipelineDetailMetadata,
  PipelineKpiBand,
  PipelineRunState,
  PipelineSection,
  PIPELINE_TYPE_TONE,
  pipelineFlatTableClassName,
} from './Surfaces';

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
  const connectorNames = useConnectorNames();
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
        sourceStream: pipeline.sourceStream ?? '',
        targetStream: pipeline.targetStream ?? '',
        functionSteps: pipeline.functionSteps ?? {},
        cron: pipeline.cron ?? 'every:5m',
        lookbackSecs: pipeline.lookbackSecs ?? 300,
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
      toast.success(t('flows.edit.toast_deleted'));
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
    ...(runs[0]?.state ? { lastRunState: runs[0].state } : {}),
  });
  const graph = pipelineGraphFromPipeline(pipeline, type);
  const mode = executionMode(pipeline);
  const alertRecord = todaysAlertRecord(pipeline);
  const routed = isRouted(graph);
  const first = firstStreamIndex(graph.sinks);
  // What the events reach: every stream (the first one as a routing step picks it), every connector,
  // every extend table; an alert pipeline's events go back to the incident they open.
  const destinations =
    mode === 'alert'
      ? [t(graph.alert.stage === 'notify' ? 'alert_mode.write_back_notify' : 'alert_mode.write_back')]
      : graph.sinks.map((sink, index) => {
          const connector = connectorOf(sink);
          const table = tableOf(sink);
          if (connector !== null) return connectorNames[connector] ?? connector;
          if (table !== null) return t('table_sink.named', { name: table });
          return routed && index === first ? t('flow.routed_detail') : sink;
        });
  const {
    lastRun,
    runs24h,
    successRate,
    processedRows,
    averageDuration,
    completedRuns,
  } = summarizePipelineRuns(runs);

  const auditReader = auditReaderName(pipeline);

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
                  // Realtime and alert pipelines run on what arrives: there is no window to read again.
                  disabled={runAccess.disabled || mode !== 'scheduled'}
                  disabledReason={
                    mode === 'realtime'
                      ? t('realtime.no_backfill')
                      : mode === 'alert'
                        ? t('alert_mode.no_backfill')
                        : runAccess.reason
                  }
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
                  {t('flows.edit.delete')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
      />
      <PageBody
        padded={false}
        className="!min-h-0 h-[calc(100vh-var(--topbar-h)-var(--pageheader-h,0px)-var(--contextbar-h,0px))] overflow-hidden"
      >
        <PipelineDetailMetadata
          health={health}
          healthLabel={t(`overview.health.${health}`)}
          typeTone={mode === 'alert' ? 'red' : PIPELINE_TYPE_TONE[type]}
          typeLabel={mode === 'alert' ? t('alert_mode.signal') : t(`filters.${type}`)}
          items={[
            {
              label: t('detail.metadata.schedule'),
              value:
                mode === 'alert'
                  ? t(graph.alert.stage === 'notify' ? 'alert_mode.schedule_notify' : 'alert_mode.schedule')
                  : mode === 'realtime'
                    ? t('realtime.title')
                    : formatSchedule(pipeline.cron, i18n.language),
            },
            {
              label: t('detail.metadata.lookback'),
              value: mode === 'scheduled' ? formatLookback(pipeline.lookbackSecs, i18n.language) : '—',
            },
            {
              label: t('detail.metadata.last_run'),
              value: formatRelativeMicros(lastRun?.startedAt, i18n.language),
            },
            ...(auditReader ? [{ label: t('detail.metadata.audit_reader'), value: auditReader }] : []),
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
          <ModuleTabs
            label={t('detail.tabs.label')}
            className="mx-0 mb-0"
            value={activeTab}
            onValueChange={setActiveTab}
            tabIdPrefix="pipeline-tab"
            items={(['overview', 'topology', 'runs', 'configuration'] as const).map((tab) => ({
              key: tab,
              label: t(`detail.tabs.${tab}`),
            }))}
          />

          <TabsContent value="overview" aria-labelledby="pipeline-tab-overview" className="m-0 min-h-0 flex-1 overflow-auto py-[12px]">
            <div className="mx-auto flex w-full max-w-[2200px] flex-col gap-[12px]">
              <PipelineKpiBand
                items={pipelineKpis(
                  alertRecord,
                  { lastRun, runs24h, successRate, processedRows, averageDuration, completedRuns },
                  t,
                  i18n.language,
                )}
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
                <PipelineFlow model={graph} connectorNames={connectorNames} />
              </PipelineSection>

              <PipelineLimits pipelineId={pipeline.id} graph={graph} />

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

          <TabsContent value="topology" aria-labelledby="pipeline-tab-topology" className="m-0 min-h-0 flex-1 overflow-auto py-[12px]">
            <div className="mx-auto w-full max-w-[2200px]">
              <PipelineSection
                title={t('detail.graph')}
                showHeaderDivider={false}
                actions={
                  <span className="font-sans text-xs text-tx-3">
                    {t('workspace.graph_stats', pipelineGraphStats(graph))}
                  </span>
                }
              >
                <PipelineFlow model={graph} connectorNames={connectorNames} />
              </PipelineSection>
            </div>
          </TabsContent>

          <TabsContent value="runs" aria-labelledby="pipeline-tab-runs" className="m-0 min-h-0 flex-1 overflow-auto py-[12px]">
            <div className="mx-auto w-full max-w-[2200px] overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]">
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

          <TabsContent value="configuration" aria-labelledby="pipeline-tab-configuration" className="m-0 min-h-0 flex-1 overflow-auto py-[12px]">
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
                        className={`rounded-sm px-3 py-1.5 font-sans text-xs font-strong ${
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
                    <PipelineConfigSection title={t('flow.source')}>
                      <PipelineConfigValue>
                        {mode === 'alert' ? t('alert_mode.source_title') : graph.sources[0]}
                      </PipelineConfigValue>
                    </PipelineConfigSection>
                    <PipelineConfigSection title={t('flow.destinations')}>
                      {destinations.map((destination) => (
                        <PipelineConfigValue key={destination}>{destination}</PipelineConfigValue>
                      ))}
                    </PipelineConfigSection>
                    <PipelineConfigSection title={t('flow.processing')}>
                      {graph.transforms.length === 0 && (
                        <PipelineConfigValue>
                          {t(mode === 'alert' ? 'alert_mode.no_steps' : 'flow.no_steps')}
                        </PipelineConfigValue>
                      )}
                      {graph.transforms.map((transform, index) => (
                        <PipelineConfigValue key={`${transform.name}-${index}`}>
                          {transform.name} · {t(`steps.types.${stepType(transform)}`)}
                        </PipelineConfigValue>
                      ))}
                    </PipelineConfigSection>
                  </div>
                ) : (
                  <JsonView
                    value={pipeline}
                    label="JSON"
                    ariaLabel={t('detail.configuration')}
                    minHeight={520}
                    maxHeight={720}
                    showHeader
                    showStatus
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
        title={t('flows.edit.delete_confirm_title')}
        description={t('flows.edit.delete_confirm_description')}
        confirmLabel={t('flows.edit.delete_confirm_label')}
        busy={remove.isPending}
        disabled={deleteAccess.disabled}
        disabledReason={deleteAccess.reason}
        onConfirm={() => deleteAccess.allowed && remove.mutate()}
      />
    </>
  );

}
