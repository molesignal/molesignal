import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ArrowRight,
  ChevronDown,
  Copy,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Settings2,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useSearchParams } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as streamsApi from '@/api/streams';
import { formatMicrosActive } from '@/lib/time';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { type ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton, Dot, Pill, QueryInput, type PillTone } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRadio,
  FormRow,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
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
import { formatRelativeMicros } from '@/time/relative';

import {
  type DisplayStream,
  type DisplayStreamVariant,
  groupStreamsByName,
  selectStreamVariant,
} from './model';

type StreamType = streamsApi.StreamType;
type StreamTab = 'all' | 'logs' | 'metrics' | 'traces' | 'profiles';
type StatusFilter = 'all' | streamsApi.StreamRuntimeStatus;

const TYPE_TONE: Record<StreamType, 'orange' | 'blue' | 'green' | 'dim'> = {
  logs: 'orange',
  metrics: 'blue',
  traces: 'green',
  profiles: 'blue',
  extend: 'dim',
};

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

function streamTypeLabel(t: (key: string) => string, type: StreamType): string {
  if (type === 'logs' || type === 'metrics' || type === 'traces') return t(`list.tabs.${type}`);
  if (type === 'profiles') return 'Profiles';
  return 'Extend';
}

const STREAM_TABS = ['all', 'logs', 'metrics', 'traces', 'profiles'] as const satisfies readonly StreamTab[];

const STATUS_FILTERS = [
  'all',
  'healthy',
  'idle',
  'delayed',
  'interrupted',
  'unused',
  'unknown',
] as const satisfies readonly StatusFilter[];

const QUERY_ACTION_CLASS =
  'inline-flex h-8 shrink-0 items-center gap-1.5 px-1.5 font-sans text-xs font-strong text-tx-1 transition-colors duration-fast hover:text-indigo-soft focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo disabled:cursor-not-allowed disabled:text-tx-4';

function streamExplorePath(streamName: string, variant: DisplayStreamVariant): string {
  const encoded = encodeURIComponent(streamName);
  // 不可查询的源 stream 在查询界面里被隐藏，跳转查询页没有意义——直接进设置页，
  // 用户可在那里查看/切换 queryable。
  if (!variant.queryable) return `/streams/${encodeURIComponent(variant.id)}`;
  if (variant.type === 'logs') return `/logs?stream=${encoded}`;
  if (variant.type === 'metrics') return `/metrics?metric=${encoded}`;
  if (variant.type === 'traces') return `/traces?stream=${encoded}`;
  return `/streams/${encodeURIComponent(variant.id)}`;
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

function formatCount(value: number): string {
  if (value < 1_000) return Math.round(value).toLocaleString();
  if (value < 1_000_000) return `${(value / 1_000).toFixed(1)}K`;
  if (value < 1_000_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  return `${(value / 1_000_000_000).toFixed(2)}B`;
}

function formatRowRate(stream: DisplayStream, windowSecs: number): string {
  const status = stream.runtime?.status;
  if (status !== 'healthy' && status !== 'delayed') return '—';
  const rows = stream.runtime?.rows;
  if (rows == null || rows <= 0 || windowSecs <= 0) return '—';
  const perSecond = rows / windowSecs;
  const noun =
    stream.types.length > 1
      ? 'items'
      : stream.types[0] === 'metrics'
      ? 'points'
      : stream.types[0] === 'traces'
        ? 'spans'
        : stream.types[0] === 'profiles'
          ? 'samples'
          : 'events';
  if (perSecond >= 1) return `${formatCount(perSecond)} ${noun}/s`;
  const perMinute = perSecond * 60;
  if (perMinute >= 1) return `${formatCount(perMinute)} ${noun}/min`;
  return `${formatCount(perMinute * 60)} ${noun}/h`;
}

function formatRetentionDays(days: number[]): string {
  return days.join(' / ');
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

export function Streams() {
  const { t, i18n } = useTranslation('streams');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const queryClient = useQueryClient();
  const createAccess = useActionAccess({
    permission: 'streams.create',
  });
  const [tab, setTab] = React.useState<StreamTab>('all');
  const [statusFilter, setStatusFilter] = React.useState<StatusFilter>('all');
  const [filter, setFilter] = React.useState('');
  const [creating, setCreating] = React.useState(() => searchParams.get('create') === '1');

  React.useEffect(() => {
    if (searchParams.get('create') !== '1') return;
    if (createAccess.allowed) setCreating(true);
    const next = new URLSearchParams(searchParams);
    next.delete('create');
    setSearchParams(next, { replace: true });
  }, [createAccess.allowed, searchParams, setSearchParams]);

  const listQuery = useQuery({
    queryKey: ['streams', 'list'],
    queryFn: () => streamsApi.list(),
  });
  const runtimeQuery = useQuery({
    queryKey: ['streams', 'runtime', 24 * 60 * 60],
    queryFn: () => streamsApi.runtimeOverview({ windowSecs: 24 * 60 * 60, bucketCount: 24 }),
    refetchInterval: 60_000,
  });

  const streams: DisplayStream[] = React.useMemo(
    () => groupStreamsByName(listQuery.data ?? [], runtimeQuery.data?.streams ?? []),
    [listQuery.data, runtimeQuery.data],
  );

  const filtered = streams.filter(
    (s) =>
      (tab === 'all' || s.types.includes(tab)) &&
      (statusFilter === 'all' || (s.runtime?.status ?? 'unknown') === statusFilter) &&
      `${s.name} ${s.description}`.toLowerCase().includes(filter.toLowerCase()),
  );
  const activeType = tab === 'all' ? undefined : tab;
  const openStreamDetail = React.useCallback(
    (stream: DisplayStream, variant?: DisplayStreamVariant) => {
      const selected = variant ?? selectStreamVariant(stream, activeType);
      if (!selected) return;
      navigate(`/streams/${encodeURIComponent(selected.id)}`);
    },
    [activeType, navigate],
  );

  const exploreStream = React.useCallback(
    (stream: DisplayStream, variant?: DisplayStreamVariant) => {
      const selected = variant ?? selectStreamVariant(stream, activeType, true);
      if (!selected) return;
      navigate(streamExplorePath(stream.name, selected));
    },
    [activeType, navigate],
  );

  const refresh = React.useCallback(() => {
    void Promise.all([listQuery.refetch(), runtimeQuery.refetch()]);
  }, [listQuery, runtimeQuery]);

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
        : state === 'empty'
          ? {
              variant: 'empty',
              title: t('list.empty_title'),
              description: t('list.empty_description'),
              action: (
                <ChromeButton
                  variant="primary"
                  disabled={createAccess.disabled}
                  disabledReason={createAccess.reason}
                  onClick={() => createAccess.allowed && setCreating(true)}
                >
                  <Plus className="h-3 w-3" /> {t('list.create')}
                </ChromeButton>
              ),
            }
          : null;

  const healthyCount = streams.filter((item) => item.runtime?.status === 'healthy').length;
  const attentionCount = streams.filter((item) => {
    const status = item.runtime?.status ?? 'unknown';
    return status === 'delayed' || status === 'interrupted' || status === 'unknown';
  }).length;
  const rows24h = streams.reduce((sum, item) => sum + (item.runtime?.rows ?? 0), 0);
  const stored24h = streams.reduce((sum, item) => sum + (item.runtime?.stored_bytes ?? 0), 0);
  const currentStored = streams.reduce(
    (sum, item) => sum + (item.runtime?.current_stored_bytes ?? 0),
    0,
  );
  const runtimeWindowSecs = runtimeQuery.data?.window_secs ?? 24 * 60 * 60;
  const generatedAt = runtimeQuery.data?.generated_at_micros;

  return (
    <>
      <ListPage
        title={t('title')}
        subtitle={t('subtitle') as string}
        kpis={[
          {
            label: t('list.kpis.healthy'),
            value: runtimeQuery.isLoading ? '—' : healthyCount,
            sub: t('list.kpis.healthy_sub', { total: streams.length }),
            tone: healthyCount === streams.length && streams.length > 0 ? 'good' : 'neutral',
          },
          {
            label: t('list.kpis.attention'),
            value: runtimeQuery.isLoading ? '—' : attentionCount,
            sub: t('list.kpis.attention_sub'),
            tone: attentionCount > 0 ? 'warn' : 'good',
          },
          {
            label: t('list.kpis.compressed_24h'),
            value: runtimeQuery.isLoading ? '—' : formatBytes(stored24h),
            sub: t('list.kpis.rows_24h', { count: formatCount(rows24h) }),
          },
          {
            label: t('list.kpis.current_storage'),
            value: runtimeQuery.isLoading ? '—' : formatBytes(currentStored),
            sub: t('list.kpis.current_storage_sub'),
          },
        ]}
        toolbar={
          <>
            <ChromeButton onClick={refresh} disabled={listQuery.isFetching || runtimeQuery.isFetching}>
              <RefreshCw className="h-3 w-3" /> {t('list.refresh')}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              disabled={createAccess.disabled}
              disabledReason={createAccess.reason}
              onClick={() => createAccess.allowed && setCreating(true)}
            >
              <Plus className="h-3 w-3" /> {t('list.create')}
            </ChromeButton>
          </>
        }
        filters={
          <div className="flex w-full flex-wrap items-center gap-3">
            <div className="flex gap-1 rounded-md border border-bd-0 bg-bg-2 p-0.5">
              {STREAM_TABS.map((kind) => (
                <button
                  key={kind}
                  onClick={() => setTab(kind)}
                  className={`rounded px-2.5 py-1 font-sans text-xs font-strong ${
                    tab === kind ? 'bg-bg-4 text-tx-0' : 'text-tx-2 hover:text-tx-0'
                  }`}
                >
                  {t(`list.tabs.${kind}`)}{' '}
                  <span className="ml-1 text-tx-3">
                    {kind === 'all'
                      ? streams.length
                      : streams.filter((stream) => stream.types.includes(kind)).length}
                  </span>
                </button>
              ))}
            </div>
            <QueryInput
              value={filter}
              onChange={setFilter}
              placeholder={t('list.search_placeholder') ?? ''}
              className="h-8 min-w-[180px] max-w-[280px] flex-1"
            />
            <select
              value={statusFilter}
              onChange={(event) => setStatusFilter(event.target.value as StatusFilter)}
              className="h-8 rounded-md border border-bd-1 bg-bg-1 px-2.5 font-sans text-xs font-semibold text-tx-1 outline-none"
              aria-label={t('list.status_filter')}
            >
              {STATUS_FILTERS.map((status) => (
                <option key={status} value={status}>
                  {status === 'all' ? t('list.status.all') : t(`list.status.${status}`)}
                </option>
              ))}
            </select>
            <div className="ml-auto text-right font-sans text-xs text-tx-2">
              <div>{t('list.result_count', { count: filtered.length })}</div>
              <div className="mt-0.5 text-type-micro text-tx-3">
                {runtimeQuery.isError
                  ? t('list.stats_unavailable')
                  : t('list.updated_at', {
                      time: generatedAt
                        ? formatRelativeMicros(generatedAt, locale)
                        : '—',
                    })}
              </div>
            </div>
          </div>
        }
        state={listState}
      >
        <DataTable
          rows={filtered}
          rowKey={(s) => s.key}
          onRowClick={openStreamDetail}
          columns={[
            {
              key: 'stream',
              header: t('list.columns.stream'),
              cell: (s) => (
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="truncate font-semibold text-tx-0">{s.name}</span>
                    {!s.queryable && <Pill tone="dim">{t('list.not_queryable')}</Pill>}
                  </div>
                  {s.description && (
                    <div className="mt-0.5 max-w-[260px] truncate text-xs text-tx-3">
                      {s.description}
                    </div>
                  )}
                </div>
              ),
              width: 280,
            },
            {
              key: 'type',
              header: t('list.columns.type'),
              cell: (s) => (
                <div className="flex flex-wrap items-center gap-1">
                  {s.types.map((type) => (
                    <Pill key={type} tone={TYPE_TONE[type]}>
                      {streamTypeLabel(t, type)}
                    </Pill>
                  ))}
                </div>
              ),
              width: 180,
            },
            {
              key: 'status',
              header: t('list.columns.status'),
              cell: (s) => <StreamStatus status={s.runtime?.status ?? 'unknown'} t={t} />,
              width: 128,
            },
            {
              key: 'rate',
              header: t('list.columns.rate'),
              cell: (s) => (
                <span className="font-mono text-xs tabular-nums text-tx-1">
                  {formatRowRate(s, runtimeWindowSecs)}
                </span>
              ),
              width: 156,
            },
            {
              key: 'last_received',
              header: t('list.columns.last_received'),
              cell: (s) => (
                <span
                  className="text-tx-2"
                  title={
                    s.runtime?.last_received_at_micros
                      ? formatMicrosActive(s.runtime.last_received_at_micros)
                      : undefined
                  }
                >
                  {formatRelativeMicros(
                    s.runtime?.last_received_at_micros,
                    locale,
                  )}
                </span>
              ),
              width: 120,
            },
            {
              key: 'volume_24h',
              header: t('list.columns.volume_24h'),
              cell: (s) => (
                <span className="font-mono text-xs tabular-nums text-tx-1">
                  {s.runtime?.stats_available ? formatBytes(s.runtime.stored_bytes) : '—'}
                </span>
              ),
              width: 128,
            },
            {
              key: 'storage',
              header: t('list.columns.storage'),
              cell: (s) => (
                <span className="font-mono text-xs tabular-nums text-tx-1">
                  {s.runtime?.stats_available ? formatBytes(s.runtime.current_stored_bytes) : '—'}
                </span>
              ),
              width: 120,
            },
            {
              key: 'retention',
              header: t('list.columns.retention'),
              cell: (s) =>
                s.retentionDays.length > 0
                  ? t('list.retention_days', { days: formatRetentionDays(s.retentionDays) })
                  : '—',
              width: 112,
            },
            {
              key: 'actions',
              header: t('list.columns.actions'),
              cell: (s) => {
                const queryableVariants = s.variants.filter((variant) => variant.queryable);
                return (
                  <div className="flex items-center justify-center gap-1">
                    {queryableVariants.length <= 1 ? (
                      <button
                        type="button"
                        className={QUERY_ACTION_CLASS}
                        onClick={(event) => {
                          event.stopPropagation();
                          exploreStream(s, queryableVariants[0]);
                        }}
                        disabled={queryableVariants.length === 0}
                      >
                        {t('list.query')} <ArrowRight className="h-3 w-3" />
                      </button>
                    ) : (
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <button
                            type="button"
                            className={QUERY_ACTION_CLASS}
                            onClick={(event) => event.stopPropagation()}
                          >
                            {t('list.query')} <ChevronDown className="h-3 w-3" />
                          </button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          {queryableVariants.map((variant) => (
                            <DropdownMenuItem
                              key={variant.id}
                              onSelect={() => exploreStream(s, variant)}
                            >
                              <ArrowRight className="h-3.5 w-3.5" />
                              {streamTypeLabel(t, variant.type)}
                            </DropdownMenuItem>
                          ))}
                        </DropdownMenuContent>
                      </DropdownMenu>
                    )}
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <button
                          type="button"
                          onClick={(event) => event.stopPropagation()}
                          className="grid h-8 w-8 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0"
                          aria-label={t('list.more_actions', { name: s.name })}
                        >
                          <MoreHorizontal className="h-4 w-4" />
                        </button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        {s.variants.map((variant) => (
                          <DropdownMenuItem
                            key={`detail-${variant.id}`}
                            onSelect={() => openStreamDetail(s, variant)}
                          >
                            <Settings2 className="h-3.5 w-3.5" />
                            {s.variants.length === 1
                              ? t('list.view_detail')
                              : `${streamTypeLabel(t, variant.type)} · ${t('list.view_detail')}`}
                          </DropdownMenuItem>
                        ))}
                        <DropdownMenuSeparator />
                        {s.variants.map((variant) => (
                          <DropdownMenuItem
                            key={`copy-${variant.id}`}
                            onSelect={() => {
                              void navigator.clipboard.writeText(variant.id);
                              toast.success(t('list.id_copied'));
                            }}
                          >
                            <Copy className="h-3.5 w-3.5" />
                            {s.variants.length === 1
                              ? t('list.copy_id')
                              : `${streamTypeLabel(t, variant.type)} · ${t('list.copy_id')}`}
                          </DropdownMenuItem>
                        ))}
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                );
              },
              width: 152,
              className: 'text-center',
              headerClassName: 'text-center',
            },
          ]}
        />
      </ListPage>

      <StreamDrawer
        access={createAccess}
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={() => {
          void queryClient.invalidateQueries({ queryKey: ['streams', 'list'] });
        }}
      />
    </>
  );
}

function StreamDrawer({
  access,
  open,
  onClose,
  onCreated,
}: {
  access: ActionAccess;
  open: boolean;
  onClose: () => void;
  onCreated: () => void;
}) {
  const { t } = useTranslation('streams');
  const [name, setName] = React.useState('');
  const [type, setType] = React.useState<StreamType>('logs');
  const [retention, setRetention] = React.useState('30');
  const [shards, setShards] = React.useState(4);
  const [indexFields, setIndexFields] = React.useState('service, level, trace_id');
  const [encryptedFields, setEncryptedFields] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [queryable, setQueryable] = React.useState(true);
  const createMutation = useMutation({
    mutationFn: streamsApi.create,
    onSuccess: (created) => {
      toast.success(`数据流 ${created.name} 已创建`);
      onCreated();
      onClose();
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : String(err));
    },
  });

  React.useEffect(() => {
    if (!open) {
      setName('');
      setType('logs');
      setRetention('30');
      setShards(4);
      setIndexFields('service, level, trace_id');
      setEncryptedFields('');
      setDescription('');
      setQueryable(true);
    }
  }, [open]);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!access.allowed || !name.trim()) return;
    const indexed = indexFields
      .split(',')
      .map((item) => item.trim())
      .filter(Boolean);
    const encrypted = encryptedFields
      .split(',')
      .map((item) => item.trim())
      .filter(Boolean);
    createMutation.mutate({
      name: name.trim(),
      stream_type: type,
      retention_days: Number(retention),
      // 加密字段以密文（Utf8）落盘；显式声明 FieldDef(encrypted=true)，其余字段仍由 ingest 推断。
      fields: encrypted.map((field) => ({
        name: field,
        data_type: 'utf8' as const,
        nullable: true,
        indexed: false,
        encrypted: true,
      })),
      settings: {
        description: description.trim() || null,
        queryable,
        index_rules: indexed.map((field) => ({
          field,
          enabled: true,
          index_type: 'full_text',
          condition: null,
          sdr_patterns: [],
        })),
      },
    });
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={t('drawer.title')}
      subtitle={t('drawer.subtitle')}
      footer={
        <FormSubmitFooter
          busy={createMutation.isPending}
          disabled={access.disabled}
          disabledReason={access.reason}
          invalid={!name.trim()}
          onCancel={onClose}
          submitLabel={t('list.create')}
          formId="stream-form"
        />
      }
    >
      <form id="stream-form" onSubmit={submit}>
        <fieldset
          disabled={access.disabled || createMutation.isPending}
          aria-disabled={access.disabled || undefined}
          className="contents disabled:cursor-not-allowed"
        >
        <FormSection title={t('drawer.identity')}>
          <FormField label={t('drawer.stream_name')} required hint={t('drawer.stream_name_hint')}>
            <FormInput
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('drawer.stream_name_placeholder')}
              required
            />
          </FormField>
          <FormField label={t('drawer.description')}>
            <FormTextarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={2}
              placeholder={t('drawer.description_placeholder')}
            />
          </FormField>
        </FormSection>

        <FormSection title={t('drawer.signal_type')} description={t('drawer.signal_type_description')}>
          <FormRadio
            value={type}
            onChange={setType}
            options={[
              { value: 'logs', label: t('drawer.types.logs.label'), hint: t('drawer.types.logs.hint') },
              { value: 'metrics', label: t('drawer.types.metrics.label'), hint: t('drawer.types.metrics.hint') },
              { value: 'traces', label: t('drawer.types.traces.label'), hint: t('drawer.types.traces.hint') },
            ]}
          />
        </FormSection>

        <FormSection title={t('drawer.storage')}>
          <FormRow>
            <FormField label={t('drawer.retention')} hint={t('drawer.retention_hint')}>
              <FormSelect value={retention} onChange={setRetention} options={['7', '14', '30', '90', '180', '365']} />
            </FormField>
            <FormField label={t('drawer.shards')} hint={t('drawer.shards_hint')}>
              <FormInput
                type="number"
                min={1}
                max={64}
                value={shards}
                onChange={(e) => setShards(Number(e.target.value))}
              />
            </FormField>
          </FormRow>
          <FormField label={t('drawer.indexed_fields')} hint={t('drawer.indexed_fields_hint')}>
            <FormInput value={indexFields} onChange={(e) => setIndexFields(e.target.value)} />
          </FormField>
          <FormField label={t('drawer.encrypted_fields')} hint={t('drawer.encrypted_fields_hint')}>
            <FormInput value={encryptedFields} onChange={(e) => setEncryptedFields(e.target.value)} />
          </FormField>
          <FormField label={t('drawer.queryable')} hint={t('drawer.queryable_hint')}>
            <Switch checked={queryable} onCheckedChange={setQueryable} />
          </FormField>
        </FormSection>
        </fieldset>
      </form>
    </FormDrawer>
  );
}
