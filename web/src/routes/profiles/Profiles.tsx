import { useQuery } from '@tanstack/react-query';
import {
  Activity,
  ChevronDown,
  ChevronRight,
  Flame,
  GitCompare,
  Layers3,
  Radio,
  RefreshCw,
  Server,
  Timer,
  Upload,
  X,
  type LucideIcon,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import * as profilesApi from '@/api/profiles';
import type { ProfileEntry } from '@/api/profiles';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor, type ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { TimeRangeChip } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { QueryState } from '@/shell/query/State';
import { SignalReference } from '@/shell/SignalReference';
import { Button } from '@/shell/ui/button';
import { Checkbox } from '@/shell/ui/checkbox';
import { Input } from '@/shell/ui/input';
import {
  formatCount,
  formatDuration,
  formatValue,
  topFunctions,
  type TopFunction,
  unitForProfileType,
} from '@/viz/profiles/flamebearer';
import { Flamegraph } from '@/viz/profiles/Flamegraph';

import {
  ServiceSelect,
  TruncatedNotice,
  TypeSelect,
  useProfileTypeLabel,
  useWindowMicros,
} from './shared';
import { UploadProfileDialog } from './UploadDialog';

const LIVE_INTERVAL_MS = 10_000;

export function Profiles() {
  const { t } = useTranslation('profiles');
  const navigate = useNavigate();
  const uploadAccess = useActionAccess({ permission: 'streams.write' });
  const typeLabel = useProfileTypeLabel();
  const { window: timeWindow, fromMicros, toMicros } = useWindowMicros();
  const [searchParams] = useSearchParams();
  const traceId = searchParams.get('trace_id') ?? undefined;
  const spanId = searchParams.get('span_id') ?? undefined;
  const [service, setService] = React.useState('');
  const [type, setType] = React.useState('');
  const [label, setLabel] = React.useState('');
  const [uploadOpen, setUploadOpen] = React.useState(false);
  const [live, setLive] = React.useState(false);
  const [excludedProfileIds, setExcludedProfileIds] = React.useState<Set<string>>(
    () => new Set(),
  );
  const [selectedFunction, setSelectedFunction] = React.useState<string | null>(null);

  const winKey = [timeWindow.from, timeWindow.to] as const;

  const listQuery = useQuery({
    queryKey: ['profiles-list', ...winKey, type, label, traceId ?? ''],
    queryFn: () =>
      profilesApi.list({
        from: fromMicros,
        to: toMicros,
        type: type || undefined,
        label: label || undefined,
        trace_id: traceId,
        limit: 200,
      }),
  });
  const rows = React.useMemo(() => listQuery.data ?? [], [listQuery.data]);
  const services = React.useMemo(
    () => Array.from(new Set(rows.map((row) => row.service).filter(Boolean))).sort(),
    [rows],
  );
  const tableRows = React.useMemo(
    () => (service ? rows.filter((row) => row.service === service) : rows),
    [rows, service],
  );
  const selectedRows = React.useMemo(
    () => tableRows.filter((row) => !excludedProfileIds.has(row.id)),
    [excludedProfileIds, tableRows],
  );
  const selectedProfileIds = React.useMemo(
    () => selectedRows.map((row) => row.id).sort(),
    [selectedRows],
  );
  const selectionKey = selectedProfileIds.join(',');
  const fallbackToTraceCorrelation = tableRows.length === 0 && Boolean(traceId);
  const hasTrace = tableRows.some((row) => row.trace_id);

  React.useEffect(() => {
    setExcludedProfileIds(new Set());
    setSelectedFunction(null);
  }, [label, service, spanId, timeWindow.from, timeWindow.to, traceId, type]);

  const flameQuery = useQuery({
    queryKey: [
      'profiles-flame-workbench',
      ...winKey,
      service,
      type,
      label,
      traceId ?? '',
      spanId ?? '',
      selectionKey || 'none',
    ],
    queryFn: () => {
      if (selectedProfileIds.length > 0) {
        return profilesApi.flamegraphSelection({
          profile_ids: selectedProfileIds,
          max_merge: 1000,
        });
      }
      return profilesApi.flamegraph({
        from: fromMicros,
        to: toMicros,
        service: service || undefined,
        type: type || undefined,
        label: label || undefined,
        trace_id: traceId,
        span_id: spanId,
      });
    },
    enabled: selectedProfileIds.length > 0 || fallbackToTraceCorrelation,
  });

  const refetchRef = React.useRef(() => {});
  refetchRef.current = () => {
    void listQuery.refetch();
    void flameQuery.refetch();
  };
  React.useEffect(() => {
    if (!live) return;
    const id = window.setInterval(() => refetchRef.current(), LIVE_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [live]);

  const fb = flameQuery.data?.flamebearer;
  const hotFunction = React.useMemo(
    () => (fb ? topFunctions(fb)[0] ?? null : null),
    [fb],
  );
  const totalSamples = React.useMemo(
    () => selectedRows.reduce((sum, row) => sum + (row.sample_count || 0), 0),
    [selectedRows],
  );
  const totalDuration = React.useMemo(
    () => selectedRows.reduce((sum, row) => sum + (row.duration_nanos || 0), 0),
    [selectedRows],
  );
  const selectedServices = React.useMemo(
    () => Array.from(new Set(selectedRows.map((row) => row.service).filter(Boolean))).sort(),
    [selectedRows],
  );
  const analysisService =
    service || (selectedServices.length === 1 ? selectedServices[0] : undefined);

  const empty =
    !listQuery.isLoading && !listQuery.isError && rows.length === 0 && !traceId;
  const listState = productStateFor(
    listQuery.isLoading ? 'loading' : listQuery.isError ? 'error' : null,
    { error: listQuery.error },
  );
  const emptyState: ProductStateProps | null = empty
    ? {
        variant: 'empty',
        title: t('list.empty_title'),
        description: t('list.empty_description'),
        action: uploadAccess.allowed ? (
          <>
            <Button size="sm" onClick={() => setUploadOpen(true)}>
              <Upload className="h-3.5 w-3.5" /> {t('upload.button')}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => navigate('/datasource/recommended/continuous-profiling')}
            >
              {t('upload.datasource_link')}
            </Button>
          </>
        ) : undefined,
      }
    : null;

  const toggleProfile = React.useCallback((id: string, selected: boolean) => {
    setExcludedProfileIds((previous) => {
      const next = new Set(previous);
      if (selected) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const toggleAllProfiles = React.useCallback(
    (selected: boolean) => {
      setExcludedProfileIds((previous) => {
        const next = new Set(previous);
        for (const row of tableRows) {
          if (selected) next.delete(row.id);
          else next.add(row.id);
        }
        return next;
      });
    },
    [tableRows],
  );

  const allSelected =
    tableRows.length > 0 && selectedRows.length === tableRows.length;
  const someSelected = selectedRows.length > 0 && !allSelected;

  const columns: DataTableColumn<ProfileEntry>[] = [
    {
      key: 'selected',
      header: (
        <div className="flex justify-center" onClick={(event) => event.stopPropagation()}>
          <Checkbox
            checked={allSelected ? true : someSelected ? 'indeterminate' : false}
            onCheckedChange={(checked) => toggleAllProfiles(checked === true)}
            aria-label={
              allSelected ? t('selection.select_none') : t('selection.select_all')
            }
          />
        </div>
      ),
      cell: (row) => (
        <div
          className="flex justify-center"
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => event.stopPropagation()}
        >
          <Checkbox
            checked={!excludedProfileIds.has(row.id)}
            onCheckedChange={(checked) => toggleProfile(row.id, checked === true)}
            aria-label={t('selection.row_aria', { id: row.id })}
          />
        </div>
      ),
      width: 48,
      className: 'px-2',
      headerClassName: 'px-2',
    },
    {
      key: 'service',
      header: t('list.columns.service'),
      cell: (row) => <span className="text-tx-0">{row.service}</span>,
    },
    {
      key: 'type',
      header: t('list.columns.type'),
      cell: (row) => typeLabel(row.profile_type),
      width: 140,
    },
    {
      key: 'timestamp',
      header: t('list.columns.timestamp'),
      cell: (row) => formatMicrosActive(row.timestamp),
      width: 190,
    },
    {
      key: 'duration',
      header: (
        <span title={t('list.tooltip.duration')}>{t('list.columns.duration')}</span>
      ),
      cell: (row) => formatDuration(row.duration_nanos),
      width: 110,
    },
    {
      key: 'samples',
      header: t('list.columns.samples'),
      cell: (row) => formatCount(row.sample_count),
      width: 100,
    },
    {
      key: 'total',
      header: (
        <span title={t('list.tooltip.total')}>{t('list.columns.total_value')}</span>
      ),
      cell: (row) =>
        formatValue(row.total_value, unitForProfileType(row.profile_type)),
      width: 120,
    },
  ];
  if (hasTrace) {
    columns.push({
      key: 'trace',
      header: t('list.columns.trace'),
      cell: (row) =>
        row.trace_id ? (
          <SignalReference type="trace_id" value={row.trace_id}>
            {row.trace_id.slice(0, 12)}
          </SignalReference>
        ) : (
          <span className="text-tx-3">—</span>
        ),
      width: 150,
    });
  }

  const toolbar = (
    <div className="flex items-center gap-2">
      <TimeRangeChip />
      <Button size="sm" onClick={() => navigate('/profiles/compare')}>
        <GitCompare className="h-3.5 w-3.5" /> {t('compare.title')}
      </Button>
      {uploadAccess.allowed && (
        <Button variant="outline" size="sm" onClick={() => setUploadOpen(true)}>
          <Upload className="h-3.5 w-3.5" /> {t('upload.button')}
        </Button>
      )}
    </div>
  );

  const noSelection = tableRows.length > 0 && selectedRows.length === 0;

  return (
    <>
      <ListPage
        title={t('title')}
        subtitle={t('subtitle') as string}
        toolbar={toolbar}
        state={listState ?? emptyState}
      >
        <div className="space-y-4">
          {traceId && (
            <div className="flex items-center gap-2">
              <span className="inline-flex items-center gap-2 rounded-md border border-bd-1 bg-bg-2 px-2.5 py-1 font-sans text-xs text-tx-1">
                {t('detail.metadata.trace')}:{' '}
                <code className="font-mono">{traceId.slice(0, 16)}</code>
                {spanId && (
                  <>
                    · {t('detail.metadata.span')}:{' '}
                    <code className="font-mono">{spanId.slice(0, 12)}</code>
                  </>
                )}
                <Link
                  to="/profiles"
                  className="ml-1 text-tx-3 hover:text-tx-0"
                  aria-label={t('analysis.clear_trace')}
                >
                  <X className="h-3 w-3" />
                </Link>
              </span>
            </div>
          )}

          <ProfileFilterBar
            service={service}
            services={services}
            type={type}
            label={label}
            live={live}
            refetching={listQuery.isFetching || flameQuery.isFetching}
            onServiceChange={setService}
            onTypeChange={setType}
            onLabelChange={setLabel}
            onRefresh={() => refetchRef.current()}
            onLiveChange={setLive}
          />

          <ProfileAnalysisSummary
            totalValue={fb ? formatValue(fb.numTicks, fb.units) : '—'}
            valueContext={type ? typeLabel(type) : t('analysis.mixed_types')}
            sampleCount={totalSamples}
            profileCount={flameQuery.data?.profile_count ?? selectedRows.length}
            serviceCount={selectedServices.length}
            duration={selectedRows.length > 0 ? formatDuration(totalDuration) : '—'}
            hotFunction={hotFunction}
            onSelectHotFunction={(name) => setSelectedFunction(name)}
          />

          {flameQuery.data?.truncated && <TruncatedNotice />}

          <section
            aria-label={t('analysis.workspace_aria')}
            className="overflow-hidden rounded-md border border-bd-0 bg-bg-1"
          >
            {noSelection ? (
              <QueryState
                state="empty"
                empty={
                  <div>
                    <div className="font-semibold text-tx-1">
                      {t('analysis.no_selection_title')}
                    </div>
                    <div className="mt-1 text-xs text-tx-3">
                      {t('analysis.no_selection_description')}
                    </div>
                  </div>
                }
                className="min-h-[360px]"
              />
            ) : flameQuery.isLoading ? (
              <QueryState
                state="loading"
                loadingLabel={t('analysis.loading_flamegraph')}
                className="min-h-[360px]"
              />
            ) : flameQuery.isError ? (
              <QueryState
                state="error"
                error={flameQuery.error}
                className="min-h-[360px]"
              />
            ) : fb ? (
              <Flamegraph
                flamebearer={fb}
                selectedFunction={selectedFunction}
                onSelectedFunctionChange={setSelectedFunction}
                service={analysisService}
                onCompare={() => navigate('/profiles/compare')}
                headerExtra={
                  tableRows.length > 0 ? (
                    <span>
                      {t('analysis.selection_summary', {
                        selected:
                          flameQuery.data?.profile_count ?? selectedRows.length,
                        total: tableRows.length,
                      })}
                    </span>
                  ) : (
                    <span>
                      {t('flamegraph.profiles_merged', {
                        count: flameQuery.data?.profile_count ?? 0,
                      })}
                    </span>
                  )
                }
                className="rounded-none border-0"
              />
            ) : (
              <QueryState
                state="empty"
                emptyLabel={t('flamegraph.no_data_description')}
                className="min-h-[360px]"
              />
            )}
          </section>

          {tableRows.length > 0 && (
            <ProfileSelectionPanel
              rows={tableRows}
              selectedCount={selectedRows.length}
              columns={columns}
              onRowClick={(row) =>
                navigate(`/profiles/${encodeURIComponent(row.id)}`)
              }
            />
          )}
        </div>
      </ListPage>
      {uploadAccess.allowed && (
        <UploadProfileDialog
          open={uploadOpen}
          onOpenChange={setUploadOpen}
          defaultService={service}
          onUploaded={() => refetchRef.current()}
        />
      )}
    </>
  );
}

function ProfileFilterBar({
  service,
  services,
  type,
  label,
  live,
  refetching,
  onServiceChange,
  onTypeChange,
  onLabelChange,
  onRefresh,
  onLiveChange,
}: {
  service: string;
  services: string[];
  type: string;
  label: string;
  live: boolean;
  refetching: boolean;
  onServiceChange: (value: string) => void;
  onTypeChange: (value: string) => void;
  onLabelChange: (value: string) => void;
  onRefresh: () => void;
  onLiveChange: (live: boolean) => void;
}) {
  const { t } = useTranslation('profiles');
  return (
    <section
      aria-label={t('analysis.filter_bar_aria')}
      className="flex min-w-0 flex-wrap items-end gap-3 rounded-md border border-bd-0 bg-bg-1 px-4 py-3"
    >
      <FilterControl label={t('filters.service')}>
        <ServiceSelect
          value={service}
          services={services}
          onChange={onServiceChange}
        />
      </FilterControl>
      <FilterControl label={t('filters.type')}>
        <TypeSelect value={type} onChange={onTypeChange} />
      </FilterControl>
      <label className="flex min-w-[180px] flex-1 flex-col gap-1.5 lg:max-w-[260px]">
        <span className="font-sans text-xs font-semibold text-tx-3">
          {t('filters.label')}
        </span>
        <Input
          value={label}
          onChange={(event) => onLabelChange(event.target.value)}
          placeholder={t('filters.label_placeholder')}
          aria-label={t('filters.label')}
          className="h-8 font-mono text-xs"
        />
      </label>
      <div className="ml-auto flex items-center gap-2">
        <Button variant="outline" size="sm" onClick={onRefresh}>
          <RefreshCw className={cn('h-3.5 w-3.5', refetching && 'animate-spin')} />
          {t('refresh')}
        </Button>
        <Button
          variant={live ? 'default' : 'outline'}
          size="sm"
          onClick={() => onLiveChange(!live)}
          title={t('live.hint')}
          aria-pressed={live}
        >
          <Radio className={cn('h-3.5 w-3.5', live && 'animate-pulse')} />
          {t('live.label')}
        </Button>
      </div>
    </section>
  );
}

function FilterControl({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="font-sans text-xs font-semibold text-tx-3">{label}</span>
      {children}
    </div>
  );
}

function ProfileAnalysisSummary({
  totalValue,
  valueContext,
  sampleCount,
  profileCount,
  serviceCount,
  duration,
  hotFunction,
  onSelectHotFunction,
}: {
  totalValue: string;
  valueContext: string;
  sampleCount: number;
  profileCount: number;
  serviceCount: number;
  duration: string;
  hotFunction: TopFunction | null;
  onSelectHotFunction: (name: string) => void;
}) {
  const { t } = useTranslation('profiles');
  return (
    <section className="overflow-hidden rounded-md border border-bd-0 bg-bg-1">
      <dl className="grid sm:grid-cols-2 xl:grid-cols-4">
        <SummaryMetric
          icon={Activity}
          iconClassName="text-orange-soft"
          label={t('analysis.total_value')}
          value={totalValue}
          sub={`${valueContext} · ${t('analysis.samples', {
            count: formatCount(sampleCount),
          })}`}
        />
        <SummaryMetric
          icon={Layers3}
          iconClassName="text-blue-soft"
          label={t('analysis.merged_profiles')}
          value={formatCount(profileCount)}
          sub={t('analysis.exact_selection')}
        />
        <SummaryMetric
          icon={Server}
          iconClassName="text-green-soft"
          label={t('analysis.services')}
          value={formatCount(serviceCount)}
          sub={t('analysis.active_selection')}
        />
        <SummaryMetric
          icon={Timer}
          iconClassName="text-purple-soft"
          label={t('analysis.duration')}
          value={duration}
          sub={t('analysis.capture_duration')}
        />
      </dl>

      <button
        type="button"
        disabled={!hotFunction}
        onClick={() => hotFunction && onSelectHotFunction(hotFunction.name)}
        className="grid min-h-12 w-full grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-3 border-t border-bd-0 px-4 py-2.5 text-left font-sans text-xs hover:bg-bg-2 disabled:cursor-default disabled:hover:bg-transparent"
      >
        <Flame className="h-4 w-4 text-orange-soft" />
        <span className="min-w-0">
          <span className="mr-2 font-semibold text-tx-3">
            {t('analysis.hotspot')}
          </span>
          <span
            className="truncate font-mono font-semibold text-tx-0"
            title={hotFunction?.name}
          >
            {hotFunction?.name ?? t('analysis.no_hotspot')}
          </span>
        </span>
        <span className="whitespace-nowrap text-tx-2">
          {t('analysis.hot_self')}:{' '}
          <strong className="font-semibold text-tx-0">
            {hotFunction ? `${hotFunction.selfPct.toFixed(1)}%` : '—'}
          </strong>
        </span>
        <span className="whitespace-nowrap text-tx-2">
          {t('analysis.hot_total')}:{' '}
          <strong className="font-semibold text-tx-0">
            {hotFunction ? `${hotFunction.totalPct.toFixed(1)}%` : '—'}
          </strong>
        </span>
      </button>
    </section>
  );
}

function SummaryMetric({
  icon: Icon,
  iconClassName,
  label,
  value,
  sub,
}: {
  icon: LucideIcon;
  iconClassName: string;
  label: string;
  value: string;
  sub: string;
}) {
  return (
    <div className="flex min-w-0 gap-3 border-b border-bd-0 px-4 py-3 last:border-b-0 sm:[&:nth-last-child(-n+2)]:border-b-0 xl:border-b-0 xl:border-r xl:last:border-r-0">
      <div className="mt-0.5 grid h-8 w-8 shrink-0 place-items-center rounded-md border border-bd-0 bg-bg-2">
        <Icon className={cn('h-4 w-4', iconClassName)} />
      </div>
      <div className="min-w-0">
        <dt className="font-sans text-xs font-semibold text-tx-3">{label}</dt>
        <dd className="mt-0.5 truncate font-mono text-lg font-semibold tabular-nums text-tx-0">
          {value}
        </dd>
        <div className="mt-0.5 truncate font-sans text-xs text-tx-2" title={sub}>
          {sub}
        </div>
      </div>
    </div>
  );
}

function ProfileSelectionPanel({
  rows,
  selectedCount,
  columns,
  onRowClick,
}: {
  rows: ProfileEntry[];
  selectedCount: number;
  columns: DataTableColumn<ProfileEntry>[];
  onRowClick: (row: ProfileEntry) => void;
}) {
  const { t } = useTranslation('profiles');
  const [expanded, setExpanded] = React.useState(false);
  return (
    <section className="overflow-hidden rounded-md border border-bd-0 bg-bg-1">
      <button
        type="button"
        onClick={() => setExpanded((value) => !value)}
        className="flex min-h-12 w-full items-center gap-3 px-4 py-2.5 text-left hover:bg-bg-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo"
        aria-expanded={expanded}
      >
        {expanded ? (
          <ChevronDown className="h-4 w-4 shrink-0 text-tx-3" />
        ) : (
          <ChevronRight className="h-4 w-4 shrink-0 text-tx-3" />
        )}
        <span className="font-sans text-sm font-semibold text-tx-0">
          {t('selection.title')}
        </span>
        <span className="font-sans text-xs text-tx-2">
          {t('selection.selected_summary', {
            selected: selectedCount,
            total: rows.length,
          })}
        </span>
        <span className="ml-auto font-sans text-xs text-tx-3">
          {expanded ? t('selection.collapse') : t('selection.expand')}
        </span>
      </button>

      {expanded && (
        <div className="border-t border-bd-0">
          <div className="border-b border-bd-0 bg-bg-2 px-4 py-2 font-sans text-xs text-tx-2">
            {t('selection.description')}
          </div>
          <div className="max-h-[360px] overflow-auto">
            <DataTable
              rows={rows}
              rowKey={(row) => row.id}
              onRowClick={onRowClick}
              columns={columns}
            />
          </div>
        </div>
      )}
    </section>
  );
}
