import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import {
  Activity,
  ArrowRight,
  Braces,
  Clock3,
  Copy,
  Database,
  Edit3,
  ExternalLink,
  MoreHorizontal,
  Plus,
  RefreshCw,
  RotateCcw,
  Save,
  Search,
  Settings2,
  Trash2,
  Workflow,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useParams } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as alertsApi from '@/api/alerts';
import * as fieldMaskingApi from '@/api/fieldMasking';
import * as ingestionApi from '@/api/ingestion';
import * as pipelinesApi from '@/api/pipelines';
import * as savedViewsApi from '@/api/savedViews';
import * as streamsApi from '@/api/streams';
import { formatMicrosActive } from '@/lib/time';
import {
  restrictActionAccess,
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { type ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import {
  Card,
  CardBody,
  CardHeader,
  ChromeButton,
  Dot,
  Pill,
  type PillTone,
  uiTableHeaderClass,
} from '@/shell/chrome';
import {
  FormField,
  FormInput,
  FormTextarea,
} from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { queryStateFor } from '@/shell/query/State';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import {
  datasourceLinkForStream,
  ingestPathForSignal,
  isIngestSignal,
} from './datasourceLink';
import { FieldEditDrawer } from './fieldEditor/FieldEditDrawer';
import { INDEX_OPTIONS, toFieldDrafts, type FieldDraft } from './fieldEditor/model';
import { logicalFieldType, streamVariantsForDetail } from './model';

type DetailTab = 'overview' | 'schema' | 'retention' | 'usage' | 'settings';

const MOLESIGNAL_SYSTEM_STREAM = '_molesignal';

interface RetentionRuleDraft {
  id: string;
  name: string;
  expression: string;
  retentionDays: string;
  enabled: boolean;
}

interface StreamDraft {
  fields: FieldDraft[];
  description: string;
  retentionDays: string;
  retentionFilter: string;
  retentionRules: RetentionRuleDraft[];
  maxQueryRange: string;
  flattenLevel: string;
  useStats: boolean;
  storeOriginal: boolean;
  distinctValues: boolean;
  queryable: boolean;
}

interface UsageSummary {
  pipelines: pipelinesApi.ScheduledPipeline[];
  views: savedViewsApi.SavedView[];
  alerts: Awaited<ReturnType<typeof alertsApi.list>>;
  unavailable: number;
}

const DETAIL_TABS: Array<{ id: DetailTab; labelKey: string; icon: React.ElementType }> = [
  { id: 'overview', labelKey: 'explore.tabs.overview', icon: Activity },
  { id: 'schema', labelKey: 'explore.tabs.schema', icon: Braces },
  { id: 'retention', labelKey: 'explore.tabs.retention', icon: Clock3 },
  { id: 'usage', labelKey: 'explore.tabs.usage', icon: Workflow },
  { id: 'settings', labelKey: 'explore.tabs.settings', icon: Settings2 },
];

const STATUS_TONE: Record<streamsApi.StreamRuntimeStatus, PillTone> = {
  healthy: 'green',
  idle: 'dim',
  delayed: 'yellow',
  interrupted: 'red',
  unused: 'dim',
  unknown: 'orange',
};

const STATUS_DOT: Record<
  streamsApi.StreamRuntimeStatus,
  NonNullable<React.ComponentProps<typeof Dot>['tone']>
> = {
  healthy: 'green',
  idle: 'dim',
  delayed: 'yellow',
  interrupted: 'red',
  unused: 'dim',
  unknown: 'orange',
};

function queryPath(stream: streamsApi.StreamSummary): string {
  const encoded = encodeURIComponent(stream.name);
  if (stream.stream_type === 'logs') return `/logs?stream=${encoded}`;
  if (stream.stream_type === 'metrics') return `/metrics?metric=${encoded}`;
  if (stream.stream_type === 'traces') return `/traces?stream=${encoded}`;
  if (stream.stream_type === 'profiles') return `/profiles?stream=${encoded}`;
  return `/streams/${encodeURIComponent(stream.id)}`;
}

function streamTypeTone(type: streamsApi.StreamType): PillTone {
  if (type === 'logs') return 'orange';
  if (type === 'metrics') return 'blue';
  if (type === 'traces') return 'green';
  if (type === 'profiles') return 'purple';
  return 'dim';
}

function streamTypeLabel(
  t: TFunction<'streams'>,
  type: streamsApi.StreamType,
): string {
  if (type === 'profiles') return 'Profiles';
  if (type === 'extend') return t('list.tabs.extend', { defaultValue: '扩展表' });
  return t(`list.tabs.${type}`);
}

function toDraft(stream: streamsApi.StreamSummary): StreamDraft {
  return {
    fields: toFieldDrafts(stream),
    description: stream.settings.description ?? '',
    retentionDays: stream.retention ? String(stream.retention.days) : '',
    retentionFilter: stream.settings.retention_filter ?? '',
    retentionRules: stream.settings.keep_conditions.map((condition, index) => ({
      id: `${condition.name}-${index}`,
      name: condition.name,
      expression: condition.expression,
      retentionDays:
        condition.retention_days == null ? '' : String(condition.retention_days),
      enabled: condition.enabled,
    })),
    maxQueryRange: stream.settings.max_query_range_hours?.toString() ?? '',
    flattenLevel: stream.settings.flatten_level?.toString() ?? '',
    useStats: stream.settings.use_stream_stats_for_partitioning,
    storeOriginal: stream.settings.store_original_data,
    distinctValues: stream.settings.enable_distinct_values,
    queryable: stream.settings.queryable,
  };
}

function parseOptionalNumber(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const number = Number(trimmed);
  return Number.isFinite(number) && number >= 0 ? number : null;
}

function payloadForDraft(
  draft: StreamDraft,
  systemStream = false,
): streamsApi.UpdateStreamSettingsRequest {
  const settings: streamsApi.StreamSettings = {
    description: draft.description.trim() || null,
    index_rules: draft.fields.map((field) => ({
      field: field.name,
      enabled: field.indexed && field.index_type !== 'none',
      index_type: field.index_type,
      condition: field.condition.trim() || null,
      sdr_patterns: field.extraction_patterns_text
        .split('\n')
        .map((item) => item.trim())
        .filter(Boolean),
    })),
    retention_filter: draft.retentionFilter.trim() || null,
    keep_conditions: draft.retentionRules.map((rule) => ({
      name: rule.name.trim(),
      expression: rule.expression.trim(),
      enabled: rule.enabled,
      retention_days: parseOptionalNumber(rule.retentionDays),
    })),
    max_query_range_hours: parseOptionalNumber(draft.maxQueryRange),
    flatten_level: parseOptionalNumber(draft.flattenLevel),
    use_stream_stats_for_partitioning: draft.useStats,
    store_original_data: draft.storeOriginal,
    enable_distinct_values: draft.distinctValues,
    queryable: draft.queryable,
    field_masking: draft.fields.flatMap((field) => {
      if (field.masking_mode === 'inherit') return [];
      return [{
        field: field.name,
        algorithm: field.masking_mode === 'custom' ? field.masking_algorithm : null,
      }];
    }),
  };
  return {
    ...(systemStream
      ? {}
      : {
          retention_days: draft.retentionDays.trim()
            ? Number(draft.retentionDays)
            : null,
        }),
    fields: draft.fields.map((field) => ({
      name: field.name,
      indexed: field.indexed,
      index_type: field.index_type,
      condition: field.condition.trim() || null,
      sdr_patterns: field.extraction_patterns_text
        .split('\n')
        .map((item) => item.trim())
        .filter(Boolean),
    })),
    settings,
  };
}

function draftSignature(draft: StreamDraft | null, systemStream = false): string {
  return draft ? JSON.stringify(payloadForDraft(draft, systemStream)) : '';
}

function formatBytes(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  const abs = Math.abs(value);
  if (abs < 1024) return `${Math.round(value)} B`;
  if (abs < 1024 ** 2) return `${(value / 1024).toFixed(1)} KiB`;
  if (abs < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MiB`;
  if (abs < 1024 ** 4) return `${(value / 1024 ** 3).toFixed(2)} GiB`;
  return `${(value / 1024 ** 4).toFixed(2)} TiB`;
}

function formatCount(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  if (Math.abs(value) < 1_000) return Math.round(value).toLocaleString();
  if (Math.abs(value) < 1_000_000) return `${(value / 1_000).toFixed(1)}K`;
  if (Math.abs(value) < 1_000_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  return `${(value / 1_000_000_000).toFixed(2)}B`;
}

function formatRate(
  rows: number | null | undefined,
  windowSecs: number,
  type: streamsApi.StreamType,
): string {
  if (rows == null || rows <= 0 || windowSecs <= 0) return '—';
  const perSecond = rows / windowSecs;
  const unit =
    type === 'metrics'
      ? 'points'
      : type === 'traces'
        ? 'spans'
        : type === 'profiles'
          ? 'samples'
          : 'events';
  if (perSecond >= 1) return `${formatCount(perSecond)} ${unit}/s`;
  const perMinute = perSecond * 60;
  if (perMinute >= 1) return `${formatCount(perMinute)} ${unit}/min`;
  return `${formatCount(perMinute * 60)} ${unit}/h`;
}

function formatRelativeMicros(
  micros: number | null | undefined,
  locale: string,
  nowMicros = Date.now() * 1000,
): string {
  if (!micros) return '—';
  const seconds = Math.round((micros - nowMicros) / 1_000_000);
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  if (Math.abs(seconds) < 60) return formatter.format(seconds, 'second');
  const minutes = Math.round(seconds / 60);
  if (Math.abs(minutes) < 60) return formatter.format(minutes, 'minute');
  const hours = Math.round(minutes / 60);
  if (Math.abs(hours) < 24) return formatter.format(hours, 'hour');
  return formatter.format(Math.round(hours / 24), 'day');
}

function validateDraft(draft: StreamDraft): string | null {
  const retention = parseOptionalNumber(draft.retentionDays);
  if (draft.retentionDays.trim() && (retention == null || retention < 1 || retention > 3650)) {
    return '数据保留期必须在 1 到 3650 天之间';
  }
  for (const rule of draft.retentionRules) {
    if (!rule.name.trim()) return '每条保留规则都需要名称';
    if (!rule.expression.trim()) return `保留规则“${rule.name || '未命名'}”缺少匹配条件`;
    const days = parseOptionalNumber(rule.retentionDays);
    if (rule.retentionDays.trim() && (days == null || days < 1 || days > 3650)) {
      return `保留规则“${rule.name}”的保留天数必须在 1 到 3650 之间`;
    }
  }
  return null;
}

async function loadUsage(streamName: string): Promise<UsageSummary> {
  const [pipelinesResult, viewsResult, alertsResult] = await Promise.allSettled([
    pipelinesApi.list(),
    savedViewsApi.list(),
    alertsApi.list(),
  ]);
  return {
    pipelines:
      pipelinesResult.status === 'fulfilled'
        ? pipelinesResult.value.filter(
            (item) =>
              item.source_stream === streamName || item.target_stream === streamName,
          )
        : [],
    views:
      viewsResult.status === 'fulfilled'
        ? viewsResult.value.filter((item) => item.stream === streamName)
        : [],
    alerts:
      alertsResult.status === 'fulfilled'
        ? alertsResult.value.filter((item) => item.query.stream?.name === streamName)
        : [],
    unavailable: [pipelinesResult, viewsResult, alertsResult].filter(
      (item) => item.status === 'rejected',
    ).length,
  };
}

async function sendTestEvent(
  stream: streamsApi.StreamSummary,
): Promise<ingestionApi.IngestResult> {
  if (stream.stream_type === 'traces') {
    const startNs = Date.now() * 1_000_000;
    return ingestionApi.ingestTraces(stream.name, [
      {
        _timestamp: Math.floor(startNs / 1000),
        trace_id: crypto.randomUUID().replace(/-/g, ''),
        span_id: crypto.randomUUID().replace(/-/g, '').slice(0, 16),
        'service.name': 'molesignal-web',
        name: 'stream.test',
        kind: 1,
        start_time_unix_nano: startNs,
        end_time_unix_nano: startNs + 1_000_000,
        duration_ns: 1_000_000,
        status_code: 'OK',
        generated_by: 'molesignal-web',
      },
    ]);
  }
  if (stream.stream_type === 'metrics') {
    return ingestionApi.ingestMetrics(stream.name, [
      {
        name: 'molesignal_stream_test_total',
        value: 1,
        timestamp: new Date().toISOString(),
        tags: { stream: stream.name, generated_by: 'molesignal-web' },
      },
    ]);
  }
  if (stream.stream_type === 'logs') {
    return ingestionApi.ingestLogs(stream.name, [
      {
        timestamp: new Date().toISOString(),
        level: 'info',
        message: `MoleSignal test event for ${stream.name}`,
        stream: stream.name,
        generated_by: 'molesignal-web',
      },
    ]);
  }
  throw new Error('Profiles 暂不支持从浏览器发送测试数据');
}

function StreamStatus({
  status,
  t,
}: {
  status: streamsApi.StreamRuntimeStatus;
  t: (key: string) => string;
}) {
  return (
    <Pill tone={STATUS_TONE[status]}>
      <Dot tone={STATUS_DOT[status]} />
      {t(`list.status.${status}`)}
    </Pill>
  );
}

export function StreamExplore() {
  const { t, i18n } = useTranslation('streams');
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [tab, setTab] = React.useState<DetailTab>('overview');
  const [draft, setDraft] = React.useState<StreamDraft | null>(null);
  const [editingField, setEditingField] = React.useState<FieldDraft | null>(null);
  const [confirmDelete, setConfirmDelete] = React.useState(false);

  const streamQuery = useQuery({
    queryKey: ['streams', 'detail', id],
    queryFn: () => streamsApi.get(id ?? ''),
    enabled: Boolean(id),
  });
  const streamListQuery = useQuery({
    queryKey: ['streams', 'list'],
    queryFn: () => streamsApi.list(),
  });
  const runtimeQuery = useQuery({
    queryKey: ['streams', 'runtime', 24 * 60 * 60],
    queryFn: () =>
      streamsApi.runtimeOverview({ windowSecs: 24 * 60 * 60, bucketCount: 24 }),
    refetchInterval: 60_000,
  });
  const stream = streamQuery.data;
  const systemStream = stream?.name === MOLESIGNAL_SYSTEM_STREAM;
  const configureAccess = useActionAccess({
    permission: systemStream ? 'sys.telemetry.manage' : 'streams.configure',
  });
  const baseDeleteAccess = useActionAccess({ permission: 'streams.delete' });
  const systemImmutableReason = t('explore.system_stream_immutable');
  const deleteAccess = restrictActionAccess(
    baseDeleteAccess,
    !systemStream,
    systemImmutableReason,
  );
  const mutableSettingsAccess = restrictActionAccess(
    configureAccess,
    !systemStream,
    systemImmutableReason,
  );
  const fieldMaskingQuery = useQuery({
    queryKey: ['field-masking-effective', stream?.id],
    queryFn: () => fieldMaskingApi.effectiveForStream(stream?.id ?? ''),
    enabled: Boolean(stream?.id) && stream?.stream_type !== 'metrics',
  });
  const streamVariants = React.useMemo(
    () => (stream ? streamVariantsForDetail(stream, streamListQuery.data ?? []) : []),
    [stream, streamListQuery.data],
  );
  const runtime =
    runtimeQuery.data?.streams.find((item) => item.id === stream?.id) ?? null;

  const usageQuery = useQuery({
    queryKey: ['streams', 'usage', stream?.name],
    queryFn: () => loadUsage(stream?.name ?? ''),
    enabled: tab === 'usage' && Boolean(stream?.name),
  });

  React.useEffect(() => {
    if (!stream) return;
    setDraft(toDraft(stream));
  }, [stream]);

  const baselineSignature = React.useMemo(
    () => (stream ? draftSignature(toDraft(stream), systemStream) : ''),
    [stream, systemStream],
  );
  const currentSignature = React.useMemo(
    () => draftSignature(draft, systemStream),
    [draft, systemStream],
  );
  const dirty = Boolean(draft && stream && currentSignature !== baselineSignature);

  React.useEffect(() => {
    if (!dirty) return;
    const warn = (event: BeforeUnloadEvent) => {
      event.preventDefault();
    };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);

  const resetDraft = React.useCallback(() => {
    if (stream) setDraft(toDraft(stream));
  }, [stream]);

  const saveMutation = useMutation({
    mutationFn: async () => {
      if (!stream || !draft) throw new Error('stream not loaded');
      const validation = validateDraft(draft);
      if (validation) throw new Error(validation);
      return streamsApi.updateSettings(
        stream.id,
        payloadForDraft(draft, systemStream),
      );
    },
    onSuccess: (updated) => {
      queryClient.setQueryData(['streams', 'detail', updated.id], updated);
      setDraft(toDraft(updated));
      void queryClient.invalidateQueries({ queryKey: ['streams', 'list'] });
      void queryClient.invalidateQueries({ queryKey: ['field-masking-effective', updated.id] });
      toast.success(t('explore.toast.updated'));
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });
  const deleteMutation = useMutation({
    mutationFn: () => {
      if (!stream) throw new Error('stream not loaded');
      return streamsApi.remove(stream.id);
    },
    onSuccess: () => {
      toast.success(t('explore.toast.deleted'));
      void queryClient.invalidateQueries({ queryKey: ['streams', 'list'] });
      navigate('/streams');
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });

  const state = queryStateFor({
    isLoading: streamQuery.isLoading,
    isError: streamQuery.isError,
    data: stream ?? null,
  });
  const pageState: ProductStateProps | null =
    state === 'loading'
      ? { variant: 'loading' }
      : state === 'error'
        ? { variant: 'error', error: streamQuery.error }
        : !stream
          ? {
              variant: 'empty',
              title: t('detail.not_found_title'),
              description: t('detail.not_found_description'),
            }
          : null;

  const status = runtime?.status ?? 'unknown';
  const risks = stream && draft ? changeRisks(stream, draft, t) : [];
  const streamName = stream?.name ?? id ?? t('detail.fallback_title');

  const refresh = () => {
    void Promise.all([
      streamQuery.refetch(),
      streamListQuery.refetch(),
      runtimeQuery.refetch(),
      tab === 'usage' ? usageQuery.refetch() : Promise.resolve(),
    ]);
  };

  return (
    <>
      <DetailPage
        title={streamName}
        subtitle={
          stream?.settings.description?.trim() ||
          t('explore.subtitle', { defaultValue: '运行状态、查询效率、存储与保留策略' })
        }
        backTo="/streams"
        toolbar={
          stream ? (
            <>
              <ChromeButton
                onClick={refresh}
                disabled={streamQuery.isFetching || runtimeQuery.isFetching}
              >
                <RefreshCw className="h-3.5 w-3.5" />
                {t('explore.toolbar.refresh')}
              </ChromeButton>
              <Link to={queryPath(stream)}>
                <ChromeButton>
                  {t('explore.toolbar.query_data')}
                  <ArrowRight className="h-3.5 w-3.5" />
                </ChromeButton>
              </Link>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <ChromeButton aria-label={t('explore.toolbar.more')}>
                    <MoreHorizontal className="h-4 w-4" />
                  </ChromeButton>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem
                    onSelect={() => {
                      void navigator.clipboard.writeText(stream.id);
                      toast.success(t('explore.toolbar.id_copied'));
                    }}
                  >
                    <Copy className="h-3.5 w-3.5" />
                    {t('explore.toolbar.copy_id')}
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
                    {t('explore.toolbar.delete')}
                  </DropdownMenuItem>
                  {dirty && (
                    <>
                      <DropdownMenuSeparator />
                      <DropdownMenuItem onSelect={resetDraft}>
                        <RotateCcw className="h-3.5 w-3.5" />
                        {t('explore.toolbar.discard')}
                      </DropdownMenuItem>
                    </>
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
              <ChromeButton
                variant="primary"
                onClick={() =>
                  configureAccess.allowed && saveMutation.mutate()
                }
                disabled={
                  configureAccess.disabled || !dirty || saveMutation.isPending
                }
                disabledReason={configureAccess.reason}
              >
                <Save className="h-3.5 w-3.5" />
                {saveMutation.isPending
                  ? t('explore.toolbar.saving')
                  : t('explore.toolbar.save')}
              </ChromeButton>
            </>
          ) : null
        }
        metadata={
          stream
            ? [
                {
                  label: t('explore.metadata.type'),
                  value: (
                    <span className="inline-flex flex-wrap items-center gap-1.5 py-px">
                      {streamVariants.map((variant) => {
                        const active = variant.id === stream.id;
                        const label = streamTypeLabel(t, variant.stream_type);
                        const pill = (
                          <Pill
                            tone={streamTypeTone(variant.stream_type)}
                            className={cn(
                              !active &&
                                'opacity-60 transition-opacity hover:opacity-100',
                            )}
                          >
                            {label}
                          </Pill>
                        );
                        return active ? (
                          <span
                            key={variant.id}
                            aria-current="true"
                            title={t('explore.metadata.active_type', { type: label })}
                          >
                            {pill}
                          </span>
                        ) : (
                          <Link
                            key={variant.id}
                            to={`/streams/${encodeURIComponent(variant.id)}`}
                            aria-label={t('explore.metadata.switch_type', { type: label })}
                            title={t('explore.metadata.switch_type', { type: label })}
                            onClick={(event) => {
                              if (!dirty) return;
                              event.preventDefault();
                              toast.error(t('explore.metadata.switch_blocked'));
                            }}
                          >
                            {pill}
                          </Link>
                        );
                      })}
                    </span>
                  ),
                },
                {
                  label: t('explore.metadata.status'),
                  value: <StreamStatus status={status} t={t} />,
                },
                {
                  label: t('explore.metadata.last_received'),
                  value: runtime?.last_received_at_micros ? (
                    <span title={formatMicrosActive(runtime.last_received_at_micros)}>
                      {formatRelativeMicros(
                        runtime.last_received_at_micros,
                        i18n.language,
                      )}
                    </span>
                  ) : (
                    '—'
                  ),
                },
                {
                  label: t('explore.metadata.retention'),
                  value: t('list.retention_days', {
                    days: stream.effective_retention.days,
                  }),
                },
              ]
            : []
        }
        state={pageState}
        bodyClassName="space-y-4"
      >
        {stream && draft && (
          <>
            <div className="flex min-h-11 items-center gap-1 overflow-x-auto border-b border-bd-0">
              {DETAIL_TABS.map((item) => {
                const Icon = item.icon;
                return (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => setTab(item.id)}
                    className={cn(
                      'relative inline-flex h-11 shrink-0 items-center gap-2 px-3 font-sans text-sm font-semibold',
                      tab === item.id
                        ? 'text-tx-0 after:absolute after:inset-x-2 after:bottom-0 after:h-0.5 after:bg-indigo'
                        : 'text-tx-2 hover:text-tx-0',
                    )}
                  >
                    <Icon className="h-4 w-4" />
                    {t(item.labelKey)}
                  </button>
                );
              })}
            </div>

            {dirty && (
              <UnsavedChanges
                access={configureAccess}
                risks={risks}
                saving={saveMutation.isPending}
                onDiscard={resetDraft}
                onSave={() => saveMutation.mutate()}
              />
            )}

            {tab === 'overview' && (
              <OverviewPanel
                stream={stream}
                runtime={runtime}
                runtimeWindowSecs={runtimeQuery.data?.window_secs ?? 24 * 60 * 60}
                runtimeError={runtimeQuery.isError}
                locale={i18n.language}
                onStatsRefresh={() => void runtimeQuery.refetch()}
              />
            )}

            {tab === 'schema' && (
              <PermissionFieldset access={configureAccess}>
                <SchemaPanel
                  fields={draft.fields}
                  onEdit={(field) =>
                    configureAccess.allowed && setEditingField(field)
                  }
                />
              </PermissionFieldset>
            )}

            {tab === 'retention' && (
              <PermissionFieldset access={mutableSettingsAccess}>
                <RetentionPanel
                  draft={draft}
                  effectiveRetentionDays={stream.effective_retention.days}
                  onChange={(patch) => {
                    if (!mutableSettingsAccess.allowed) return;
                    setDraft((current) =>
                      current ? { ...current, ...patch } : current,
                    );
                  }}
                />
              </PermissionFieldset>
            )}

            {tab === 'usage' && (
              <UsagePanel
                stream={stream}
                summary={usageQuery.data ?? null}
                loading={usageQuery.isLoading}
                error={usageQuery.isError}
              />
            )}

            {tab === 'settings' && (
              <PermissionFieldset access={mutableSettingsAccess}>
                <SettingsPanel
                  draft={draft}
                  onChange={(patch) => {
                    if (!mutableSettingsAccess.allowed) return;
                    setDraft((current) =>
                      current ? { ...current, ...patch } : current,
                    );
                  }}
                />
              </PermissionFieldset>
            )}
          </>
        )}
      </DetailPage>

      <FieldEditDrawer
        access={configureAccess}
        field={editingField}
        effectiveMasking={
          fieldMaskingQuery.data?.fields.find((field) => field.field === editingField?.name) ?? null
        }
        maskingSupported={stream?.stream_type !== 'metrics'}
        onClose={() => setEditingField(null)}
        onApply={(updated) => {
          if (!configureAccess.allowed) return;
          setDraft((current) =>
            current
              ? {
                  ...current,
                  fields: current.fields.map((field) =>
                    field.name === updated.name ? updated : field,
                  ),
                }
              : current,
          );
          setEditingField(null);
        }}
      />
      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        destructive
        title={t('explore.delete_confirm_title')}
        description={t('explore.delete_confirm_description', {
          stream: streamName,
        })}
        confirmLabel={t('explore.toolbar.delete')}
        busy={deleteMutation.isPending}
        disabled={deleteAccess.disabled}
        disabledReason={deleteAccess.reason}
        onConfirm={() => deleteAccess.allowed && deleteMutation.mutate()}
      />
    </>
  );
}

function PermissionFieldset({
  access,
  children,
}: {
  access: ActionAccess;
  children: React.ReactNode;
}) {
  return (
    <fieldset
      disabled={access.disabled}
      aria-disabled={access.disabled || undefined}
      title={access.reason}
      className="contents disabled:cursor-not-allowed"
    >
      {children}
    </fieldset>
  );
}

function changeRisks(
  stream: streamsApi.StreamSummary,
  draft: StreamDraft,
  t: TFunction<'streams'>,
): string[] {
  const original = toDraft(stream);
  const risks: string[] = [];
  if (JSON.stringify(original.fields) !== JSON.stringify(draft.fields)) {
    risks.push(t('explore.changes.index_risk'));
  }
  const originalDays = stream.retention?.days ?? stream.effective_retention.days;
  const nextDays = parseOptionalNumber(draft.retentionDays);
  if (nextDays != null && nextDays < originalDays) {
    risks.push(
      t('explore.changes.retention_risk', {
        days: nextDays,
      }),
    );
  }
  if (original.queryable && !draft.queryable) {
    risks.push(t('explore.changes.query_risk'));
  }
  return risks;
}

function UnsavedChanges({
  access,
  risks,
  saving,
  onDiscard,
  onSave,
}: {
  access: ActionAccess;
  risks: string[];
  saving: boolean;
  onDiscard: () => void;
  onSave: () => void;
}) {
  const { t } = useTranslation('streams');
  return (
    <div className="sticky top-2 z-20 flex flex-wrap items-center gap-3 rounded-lg border border-yellow/35 bg-yellow-dim px-4 py-3 shadow-sm">
      <div className="min-w-0 flex-1">
        <div className="font-sans text-sm font-semibold text-tx-0">
          {t('explore.changes.unsaved')}
        </div>
        <div className="mt-0.5 font-sans text-xs text-tx-2">
          {risks.length > 0 ? risks.join(' · ') : t('explore.changes.review')}
        </div>
      </div>
      <ChromeButton size="sm" onClick={onDiscard}>
        {t('explore.toolbar.discard')}
      </ChromeButton>
      <ChromeButton
        size="sm"
        variant="primary"
        onClick={() => access.allowed && onSave()}
        disabled={access.disabled || saving}
        disabledReason={access.reason}
      >
        <Save className="h-3 w-3" />
        {saving ? t('explore.toolbar.saving') : t('explore.toolbar.save')}
      </ChromeButton>
    </div>
  );
}

function OverviewPanel({
  stream,
  runtime,
  runtimeWindowSecs,
  runtimeError,
  locale,
  onStatsRefresh,
}: {
  stream: streamsApi.StreamSummary;
  runtime: streamsApi.StreamRuntime | null;
  runtimeWindowSecs: number;
  runtimeError: boolean;
  locale: string;
  onStatsRefresh: () => void;
}) {
  const { t } = useTranslation('streams');
  const indexedFields = stream.schema.fields.filter((field) => field.indexed).length;
  const totalFields = stream.schema.fields.length;
  const noData =
    !runtimeError &&
    runtime != null &&
    runtime.rows === 0 &&
    runtime.current_stored_bytes === 0;
  const testMutation = useMutation({
    mutationFn: () => sendTestEvent(stream),
    onSuccess: (result) => {
      toast.success(
        t('explore.overview.test_sent', {
          accepted: result.accepted,
          rejected: result.rejected,
        }),
      );
      window.setTimeout(onStatsRefresh, 1500);
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });

  return (
    <div className="space-y-4">
      {runtimeError && (
        <div className="rounded-lg border border-yellow/35 bg-yellow-dim px-4 py-3 font-sans text-sm text-tx-1">
          {t('explore.overview.stats_unavailable')}
        </div>
      )}

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
        <MetricCard
          label={t('explore.kpis.events_24h')}
          value={runtime?.stats_available ? formatCount(runtime.rows) : '—'}
          note={t('explore.kpis.from_parquet_file_meta')}
        />
        <MetricCard
          label={t('explore.kpis.receive_rate')}
          value={formatRate(runtime?.rows, runtimeWindowSecs, stream.stream_type)}
          note={t('explore.kpis.average_24h')}
        />
        <MetricCard
          label={t('explore.kpis.compressed_24h')}
          value={runtime?.stats_available ? formatBytes(runtime.stored_bytes) : '—'}
          note={t('explore.kpis.compressed_note')}
        />
        <MetricCard
          label={t('explore.kpis.current_storage')}
          value={
            runtime?.stats_available ? formatBytes(runtime.current_stored_bytes) : '—'
          }
          note={t('explore.kpis.live_parquet')}
        />
        <MetricCard
          label={t('explore.kpis.index_coverage')}
          value={totalFields > 0 ? `${indexedFields} / ${totalFields}` : '—'}
          note={t('explore.kpis.index_coverage_note')}
        />
      </div>

      {noData ? (
        <div className="grid min-h-[300px] place-items-center rounded-lg border border-dashed border-bd-1 bg-bg-1 px-6 py-10 text-center">
          <div className="max-w-lg">
            <Database className="mx-auto h-8 w-8 text-tx-3" />
            <h3 className="mt-4 font-sans text-lg font-display-strong text-tx-0">
              {t('explore.overview.no_data_title')}
            </h3>
            <p className="mt-2 font-sans text-sm leading-relaxed text-tx-2">
              {t('explore.overview.no_data_description', { stream: stream.name })}
            </p>
            <div className="mt-5 flex flex-wrap justify-center gap-2">
              <Link to={datasourceLinkForStream(stream)}>
                <ChromeButton>
                  {t('explore.overview.configure_source')}
                  <ExternalLink className="h-3.5 w-3.5" />
                </ChromeButton>
              </Link>
              {stream.stream_type !== 'profiles' && (
                <ChromeButton
                  variant="primary"
                  onClick={() => testMutation.mutate()}
                  disabled={testMutation.isPending}
                >
                  {testMutation.isPending
                    ? t('explore.overview.sending_test')
                    : t('explore.overview.send_test')}
                </ChromeButton>
              )}
            </div>
          </div>
        </div>
      ) : (
        <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_320px]">
          <TrendChart buckets={runtime?.buckets ?? []} />
          <Card>
            <CardHeader title={t('explore.overview.operational_state')} />
            <CardBody className="divide-y divide-bd-0 p-0">
              <OverviewRow
                label={t('explore.metadata.status')}
                value={
                  <StreamStatus
                    status={runtime?.status ?? 'unknown'}
                    t={t}
                  />
                }
              />
              <OverviewRow
                label={t('explore.overview.first_received')}
                value={
                  runtime?.first_received_at_micros
                    ? formatRelativeMicros(runtime.first_received_at_micros, locale)
                    : '—'
                }
              />
              <OverviewRow
                label={t('explore.metadata.last_received')}
                value={
                  runtime?.last_received_at_micros
                    ? formatRelativeMicros(runtime.last_received_at_micros, locale)
                    : '—'
                }
              />
              <OverviewRow
                label={t('explore.metadata.retention')}
                value={t('list.retention_days', {
                  days: stream.effective_retention.days,
                })}
              />
              <OverviewRow
                label={t('explore.overview.queryability')}
                value={
                  stream.settings.queryable
                    ? t('explore.overview.queryable')
                    : t('explore.overview.not_queryable')
                }
              />
            </CardBody>
          </Card>
        </div>
      )}
    </div>
  );
}

function MetricCard({
  label,
  value,
  note,
}: {
  label: string;
  value: React.ReactNode;
  note?: string;
}) {
  return (
    <div className="min-h-[108px] rounded-lg border border-bd-0 bg-bg-1 px-4 py-3.5">
      <div className="font-sans text-xs font-semibold text-tx-2">{label}</div>
      <div className="mt-2.5 truncate font-sans text-2xl font-display-strong tabular-nums text-tx-0">
        {value}
      </div>
      {note && <div className="mt-1.5 truncate font-sans text-type-micro text-tx-3">{note}</div>}
    </div>
  );
}

function TrendChart({
  buckets,
}: {
  buckets: streamsApi.StreamRuntimeBucket[];
}) {
  const { t } = useTranslation('streams');
  const [metric, setMetric] = React.useState<'rows' | 'storage'>('rows');
  const values = buckets.map((bucket) =>
    metric === 'rows' ? bucket.rows : bucket.stored_bytes,
  );
  const max = Math.max(0, ...values);
  return (
    <Card>
      <CardHeader
        title={
          <div>
            <div>{t('explore.overview.trend_title')}</div>
            <div className="mt-0.5 font-sans text-xs font-normal text-tx-3">
              {t('explore.overview.trend_subtitle')}
            </div>
          </div>
        }
        actions={
          <div className="flex rounded-md border border-bd-0 bg-bg-2 p-0.5">
            {(['rows', 'storage'] as const).map((item) => (
              <button
                key={item}
                type="button"
                onClick={() => setMetric(item)}
                className={cn(
                  'rounded px-2 py-1 font-sans text-xs font-semibold',
                  metric === item ? 'bg-bg-4 text-tx-0' : 'text-tx-2',
                )}
              >
                {t(`explore.overview.metric_${item}`)}
              </button>
            ))}
          </div>
        }
      />
      <CardBody>
        {max === 0 ? (
          <div className="grid h-[220px] place-items-center font-sans text-sm text-tx-3">
            {t('explore.overview.no_chart_data')}
          </div>
        ) : (
          <TimeSeriesChart
            series={[
              {
                id: `stream-${metric}`,
                name: t(`explore.overview.metric_${metric}`),
                data: values,
                timestamps: buckets.map((bucket) =>
                  Math.round((bucket.start_micros + bucket.end_micros) / 2),
                ),
                unit: metric === 'rows' ? 'rows' : 'bytes',
              },
            ]}
            {...(buckets[0] && buckets.at(-1)
              ? {
                  xDomain: [
                    buckets[0].start_micros,
                    buckets.at(-1)!.end_micros,
                  ] as [number, number],
                }
              : {})}
            height={220}
            ariaLabel={t('explore.overview.trend_title')}
            options={{
              drawStyle: 'bar',
              showPoints: 'never',
              legendMode: 'hidden',
              leftAxis: {
                min: 0,
                unit: metric === 'rows' ? 'rows' : 'bytes',
              },
            }}
            showLegend={false}
          />
        )}
      </CardBody>
    </Card>
  );
}

function OverviewRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex min-h-12 items-center gap-4 px-4 py-3">
      <span className="font-sans text-xs text-tx-2">{label}</span>
      <span className="ml-auto text-right font-sans text-xs font-semibold text-tx-0">
        {value}
      </span>
    </div>
  );
}

function SchemaPanel({
  fields,
  onEdit,
}: {
  fields: FieldDraft[];
  onEdit: (field: FieldDraft) => void;
}) {
  const { t } = useTranslation('streams');
  const [filter, setFilter] = React.useState('');
  const needle = filter.trim().toLowerCase();
  const visibleFields = needle
    ? fields.filter((field) => field.name.toLowerCase().includes(needle))
    : fields;
  const indexed = fields.filter((field) => field.indexed).length;
  const encrypted = fields.filter((field) => field.encrypted).length;
  const suggestedField =
    fields.find((field) => field.data_type === 'utf8' && !field.indexed) ??
    fields.find((field) => !field.indexed);

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <div>
          <div className="font-sans text-sm font-semibold text-tx-0">
            {t('explore.schema.summary', {
              fields: fields.length,
              indexed,
              encrypted,
            })}
          </div>
          <div className="mt-1 font-sans text-xs text-tx-3">
            {t('explore.schema.readonly_hint')}
          </div>
        </div>
        <div className="ml-auto flex h-9 min-w-[260px] items-center gap-2 rounded-md border border-bd-1 bg-bg-1 px-3">
          <Search className="h-3.5 w-3.5 text-tx-3" />
          <input
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            placeholder={t('explore.schema.search_placeholder')}
            className="min-w-0 flex-1 bg-transparent font-sans text-sm text-tx-0 outline-none placeholder:text-tx-3"
          />
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-3 rounded-lg border border-blue/25 bg-blue-dim px-4 py-3">
        <div className="min-w-0 flex-1">
          <div className="font-sans text-sm font-semibold text-tx-0">
            {t('explore.schema.fts_title')}
          </div>
          <div className="mt-0.5 font-sans text-xs text-tx-2">
            {t('explore.schema.fts_description')}
          </div>
        </div>
        {suggestedField && (
          <ChromeButton size="sm" onClick={() => onEdit(suggestedField)}>
            {t('explore.schema.configure_index')}
          </ChromeButton>
        )}
      </div>

      <div className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
        <div className="overflow-x-auto">
          <table className="w-full min-w-[900px] border-collapse font-sans text-xs">
            <thead>
              <tr>
                <th className={cn('px-4 py-2.5 text-left', uiTableHeaderClass)}>
                  {t('explore.schema.columns.name')}
                </th>
                <th className={cn('w-28 px-4 py-2.5 text-left', uiTableHeaderClass)}>
                  {t('explore.schema.columns.type')}
                </th>
                <th className={cn('w-40 px-4 py-2.5 text-left', uiTableHeaderClass)}>
                  {t('explore.schema.columns.index_type')}
                </th>
                <th className={cn('px-4 py-2.5 text-left', uiTableHeaderClass)}>
                  {t('explore.schema.columns.condition')}
                </th>
                <th className={cn('w-44 px-4 py-2.5 text-left', uiTableHeaderClass)}>
                  {t('explore.schema.columns.extraction')}
                </th>
                <th className={cn('w-24 px-4 py-2.5 text-left', uiTableHeaderClass)}>
                  {t('explore.schema.columns.encryption')}
                </th>
                <th className={cn('w-20 px-4 py-2.5 text-right', uiTableHeaderClass)}>
                  {t('list.columns.actions')}
                </th>
              </tr>
            </thead>
            <tbody>
              {visibleFields.map((field) => (
                <tr key={field.name} className="border-t border-bd-0 hover:bg-bg-2">
                  <td className="px-4 py-3 font-mono text-xs font-semibold text-tx-0">
                    {field.name}
                  </td>
                  <td className="px-4 py-3">
                    <span title={t('explore.schema.storage_type', { type: field.data_type })}>
                      <Pill tone="blue">
                        {t(`explore.schema.field_types.${logicalFieldType(field.data_type)}`)}
                      </Pill>
                    </span>
                  </td>
                  <td className="px-4 py-3">
                    <Pill tone={field.indexed ? 'indigo' : 'dim'}>
                      {t(
                        INDEX_OPTIONS.find((option) => option.value === field.index_type)
                          ?.labelKey ?? 'explore.index_options.none',
                      )}
                    </Pill>
                  </td>
                  <td className="max-w-[300px] truncate px-4 py-3 font-mono text-xs text-tx-2">
                    {field.condition || '—'}
                  </td>
                  <td className="px-4 py-3 text-tx-2">
                    {field.extraction_patterns_text
                      ? t('explore.schema.pattern_count', {
                          count: field.extraction_patterns_text.split('\n').filter(Boolean)
                            .length,
                        })
                      : '—'}
                  </td>
                  <td className="px-4 py-3 text-tx-2">
                    {field.encrypted ? t('explore.schema.encrypted') : '—'}
                  </td>
                  <td className="px-4 py-3 text-right">
                    <button
                      type="button"
                      onClick={() => onEdit(field)}
                      className="inline-grid h-8 w-8 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0"
                      aria-label={t('explore.schema.edit_field', { name: field.name })}
                    >
                      <Edit3 className="h-3.5 w-3.5" />
                    </button>
                  </td>
                </tr>
              ))}
              {visibleFields.length === 0 && (
                <tr>
                  <td colSpan={7} className="h-32 text-center text-tx-3">
                    {fields.length === 0
                      ? t('explore.schema.no_schema')
                      : t('explore.schema.no_match')}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}

function RetentionPanel({
  draft,
  effectiveRetentionDays,
  onChange,
}: {
  draft: StreamDraft;
  effectiveRetentionDays: number;
  onChange: (patch: Partial<StreamDraft>) => void;
}) {
  const { t } = useTranslation('streams');
  const updateRule = (id: string, patch: Partial<RetentionRuleDraft>) => {
    onChange({
      retentionRules: draft.retentionRules.map((rule) =>
        rule.id === id ? { ...rule, ...patch } : rule,
      ),
    });
  };
  const addRule = () => {
    const index = draft.retentionRules.length + 1;
    onChange({
      retentionRules: [
        ...draft.retentionRules,
        {
          id: `new-${Date.now()}`,
          name: `retention_${index}`,
          expression: '',
          retentionDays: '',
          enabled: true,
        },
      ],
    });
  };

  return (
    <div className="space-y-4">
      <div className="grid gap-4 xl:grid-cols-[320px_minmax(0,1fr)]">
        <Card>
          <CardHeader title={t('explore.retention.default_policy')} />
          <CardBody className="space-y-4">
            <FormField
              label={t('explore.retention.data_retention_days')}
              hint={t('explore.retention.days_default_hint', {
                days: effectiveRetentionDays,
              })}
            >
              <FormInput
                type="number"
                min={1}
                max={3650}
                value={draft.retentionDays}
                onChange={(event) => onChange({ retentionDays: event.target.value })}
                placeholder={t('explore.retention.days_placeholder', {
                  days: effectiveRetentionDays,
                })}
              />
            </FormField>
            <div className="rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5 font-sans text-xs leading-relaxed text-tx-2">
              {t('explore.retention.default_explanation')}
            </div>
          </CardBody>
        </Card>

        <div className="min-w-0">
          <div className="mb-3 flex flex-wrap items-center gap-3">
            <div>
              <div className="font-sans text-sm font-semibold text-tx-0">
                {t('explore.retention.exception_rules')}
              </div>
              <div className="mt-1 font-sans text-xs text-tx-3">
                {t('explore.retention.exception_rules_hint')}
              </div>
            </div>
            <ChromeButton className="ml-auto" onClick={addRule}>
              <Plus className="h-3.5 w-3.5" />
              {t('explore.retention.add_rule')}
            </ChromeButton>
          </div>

          <div className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
            <div className="hidden grid-cols-[52px_52px_170px_minmax(220px,1fr)_130px_120px_44px] border-b border-bd-0 px-3 py-2 lg:grid">
              {[
                t('explore.retention.priority'),
                t('explore.retention.enabled'),
                t('explore.retention.rule_name'),
                t('explore.retention.condition'),
                t('explore.retention.retention_days'),
                t('explore.retention.estimated_share'),
                '',
              ].map((label, index) => (
                <div key={`${label}-${index}`} className={uiTableHeaderClass}>
                  {label}
                </div>
              ))}
            </div>

            {draft.retentionRules.length === 0 ? (
              <div className="grid h-36 place-items-center px-6 text-center font-sans text-sm text-tx-3">
                {t('explore.retention.no_rules')}
              </div>
            ) : (
              draft.retentionRules.map((rule, index) => (
                <div
                  key={rule.id}
                  className="grid gap-3 border-b border-bd-0 px-3 py-3 last:border-b-0 lg:grid-cols-[52px_52px_170px_minmax(220px,1fr)_130px_120px_44px] lg:items-center"
                >
                  <span className="font-mono text-xs text-tx-2">{index + 1}</span>
                  <Switch
                    checked={rule.enabled}
                    onCheckedChange={(checked) => updateRule(rule.id, { enabled: checked })}
                  />
                  <FormInput
                    value={rule.name}
                    onChange={(event) => updateRule(rule.id, { name: event.target.value })}
                    aria-label={t('explore.retention.rule_name')}
                  />
                  <FormInput
                    value={rule.expression}
                    onChange={(event) =>
                      updateRule(rule.id, { expression: event.target.value })
                    }
                    placeholder={t('explore.retention.condition_placeholder')}
                    className="font-mono"
                    aria-label={t('explore.retention.condition')}
                  />
                  <FormInput
                    type="number"
                    min={1}
                    max={3650}
                    value={rule.retentionDays}
                    onChange={(event) =>
                      updateRule(rule.id, { retentionDays: event.target.value })
                    }
                    placeholder={t('explore.retention.follow_default')}
                    aria-label={t('explore.retention.retention_days')}
                  />
                  <span className="font-sans text-xs text-tx-3">
                    {t('explore.retention.evaluate_after_save')}
                  </span>
                  <button
                    type="button"
                    onClick={() =>
                      onChange({
                        retentionRules: draft.retentionRules.filter(
                          (item) => item.id !== rule.id,
                        ),
                      })
                    }
                    className="grid h-8 w-8 place-items-center rounded-md text-tx-3 hover:bg-red-dim hover:text-red-soft"
                    aria-label={t('explore.retention.delete_rule', { name: rule.name })}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </div>
              ))
            )}
          </div>
        </div>
      </div>

      <details className="rounded-lg border border-bd-0 bg-bg-1">
        <summary className="cursor-pointer px-4 py-3 font-sans text-sm font-semibold text-tx-1">
          {t('explore.retention.advanced_filter')}
        </summary>
        <div className="border-t border-bd-0 p-4">
          <FormField
            label={t('explore.retention.keep_condition')}
            hint={t('explore.retention.keep_condition_hint')}
          >
            <FormInput
              value={draft.retentionFilter}
              onChange={(event) => onChange({ retentionFilter: event.target.value })}
              placeholder={t('explore.retention.keep_condition_placeholder')}
              className="font-mono"
            />
          </FormField>
        </div>
      </details>
    </div>
  );
}

function UsagePanel({
  stream,
  summary,
  loading,
  error,
}: {
  stream: streamsApi.StreamSummary;
  summary: UsageSummary | null;
  loading: boolean;
  error: boolean;
}) {
  const { t } = useTranslation('streams');
  if (loading) {
    return (
      <div className="grid h-56 place-items-center font-sans text-sm text-tx-3">
        {t('explore.usage.loading')}
      </div>
    );
  }

  return (
    <div className="grid gap-4 xl:grid-cols-2">
      <UsageGroup
        title={t('explore.usage.write_source')}
        subtitle={t('explore.usage.write_source_hint')}
        items={[
          {
            id: 'ingest',
            title: t('explore.usage.ingest_endpoint'),
            description: isIngestSignal(stream.stream_type)
              ? ingestPathForSignal(stream.stream_type, stream.name)
              : '—',
            to: datasourceLinkForStream(stream),
          },
        ]}
      />
      <UsageGroup
        title={t('explore.usage.pipelines')}
        subtitle={t('explore.usage.pipelines_hint')}
        items={(summary?.pipelines ?? []).map((item) => ({
          id: item.id,
          title: item.name,
          description: `${item.source_stream ?? '—'} → ${item.target_stream ?? '—'}`,
          to: `/pipelines/${encodeURIComponent(item.id)}`,
        }))}
      />
      <UsageGroup
        title={t('explore.usage.saved_queries')}
        subtitle={t('explore.usage.saved_queries_hint')}
        items={(summary?.views ?? []).map((item) => ({
          id: item.id,
          title: item.name,
          description: item.language.toUpperCase(),
          to: queryPath(stream),
        }))}
      />
      <UsageGroup
        title={t('explore.usage.alert_rules')}
        subtitle={t('explore.usage.alert_rules_hint')}
        items={(summary?.alerts ?? []).map((item) => ({
          id: item.id,
          title: item.name,
          description: item.enabled
            ? t('explore.usage.enabled')
            : t('explore.usage.disabled'),
          to: `/alerts/rules/${encodeURIComponent(item.id)}/edit`,
        }))}
      />
      {(error || (summary?.unavailable ?? 0) > 0) && (
        <div className="xl:col-span-2 rounded-lg border border-yellow/30 bg-yellow-dim px-4 py-3 font-sans text-xs text-tx-1">
          {t('explore.usage.partial_unavailable')}
        </div>
      )}
    </div>
  );
}

function UsageGroup({
  title,
  subtitle,
  items,
}: {
  title: string;
  subtitle: string;
  items: Array<{ id: string; title: string; description: string; to: string }>;
}) {
  const { t } = useTranslation('streams');
  return (
    <Card>
      <CardHeader
        title={
          <div>
            <div>{title}</div>
            <div className="mt-0.5 font-sans text-xs font-normal text-tx-3">
              {subtitle}
            </div>
          </div>
        }
        actions={<Pill tone="dim">{items.length}</Pill>}
      />
      <CardBody className="p-0">
        {items.length === 0 ? (
          <div className="grid h-28 place-items-center font-sans text-xs text-tx-3">
            {t('explore.usage.none')}
          </div>
        ) : (
          items.map((item) => (
            <Link
              key={item.id}
              to={item.to}
              className="flex min-h-14 items-center gap-3 border-b border-bd-0 px-4 py-3 last:border-b-0 hover:bg-bg-2"
            >
              <div className="min-w-0 flex-1">
                <div className="truncate font-sans text-sm font-semibold text-tx-0">
                  {item.title}
                </div>
                <div className="mt-0.5 truncate font-mono text-xs text-tx-3">
                  {item.description}
                </div>
              </div>
              <ArrowRight className="h-3.5 w-3.5 text-tx-3" />
            </Link>
          ))
        )}
      </CardBody>
    </Card>
  );
}

function SettingsPanel({
  draft,
  onChange,
}: {
  draft: StreamDraft;
  onChange: (patch: Partial<StreamDraft>) => void;
}) {
  const { t } = useTranslation('streams');
  return (
    <div className="grid gap-4 xl:grid-cols-3">
      <SettingsSection
        title={t('explore.runtime.basic')}
        description={t('explore.runtime.basic_hint')}
      >
        <FormField label={t('explore.runtime.description_label')}>
          <FormTextarea
            value={draft.description}
            onChange={(event) => onChange({ description: event.target.value })}
            rows={5}
            placeholder={t('explore.runtime.description_placeholder')}
          />
        </FormField>
      </SettingsSection>

      <SettingsSection
        title={t('explore.runtime.query_capabilities')}
        description={t('explore.runtime.query_capabilities_hint')}
      >
        <ToggleRow
          title={t('explore.runtime.queryable')}
          hint={t('explore.runtime.queryable_hint')}
          checked={draft.queryable}
          onChange={(queryable) => onChange({ queryable })}
        />
        <ToggleRow
          title={t('explore.runtime.enable_distinct_values')}
          checked={draft.distinctValues}
          onChange={(distinctValues) => onChange({ distinctValues })}
        />
        <FormField
          label={t('explore.runtime.max_query_range')}
          hint={t('explore.runtime.max_query_range_hint')}
        >
          <FormInput
            type="number"
            min={0}
            value={draft.maxQueryRange}
            onChange={(event) => onChange({ maxQueryRange: event.target.value })}
          />
        </FormField>
      </SettingsSection>

      <SettingsSection
        title={t('explore.runtime.storage_behavior')}
        description={t('explore.runtime.storage_behavior_hint')}
      >
        <ToggleRow
          title={t('explore.runtime.store_original')}
          checked={draft.storeOriginal}
          onChange={(storeOriginal) => onChange({ storeOriginal })}
        />
        <ToggleRow
          title={t('explore.runtime.use_stats')}
          checked={draft.useStats}
          onChange={(useStats) => onChange({ useStats })}
        />
        <details className="rounded-md border border-bd-0 bg-bg-2">
          <summary className="cursor-pointer px-3 py-2.5 font-sans text-xs font-semibold text-tx-1">
            {t('explore.runtime.advanced')}
          </summary>
          <div className="border-t border-bd-0 p-3">
            <FormField
              label={t('explore.runtime.flatten_level')}
              hint={t('explore.runtime.flatten_level_hint')}
            >
              <FormInput
                type="number"
                min={0}
                max={32}
                value={draft.flattenLevel}
                onChange={(event) => onChange({ flattenLevel: event.target.value })}
              />
            </FormField>
          </div>
        </details>
      </SettingsSection>
    </div>
  );
}

function SettingsSection({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-lg border border-bd-0 bg-bg-1">
      <div className="border-b border-bd-0 px-4 py-3">
        <h3 className="font-sans text-sm font-semibold text-tx-0">{title}</h3>
        <p className="mt-1 font-sans text-xs leading-relaxed text-tx-3">{description}</p>
      </div>
      <div className="space-y-4 p-4">{children}</div>
    </section>
  );
}

function ToggleRow({
  title,
  hint,
  checked,
  onChange,
}: {
  title: string;
  hint?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex items-start gap-4 rounded-md border border-bd-0 bg-bg-2 px-3 py-3">
      <div className="min-w-0">
        <div className="font-sans text-xs font-semibold text-tx-0">{title}</div>
        {hint && (
          <div className="mt-1 font-sans text-xs leading-relaxed text-tx-3">{hint}</div>
        )}
      </div>
      <Switch
        checked={checked}
        onCheckedChange={onChange}
        className="ml-auto shrink-0 data-[state=checked]:bg-indigo"
      />
    </div>
  );
}
