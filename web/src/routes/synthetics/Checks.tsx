import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import {
  Copy,
  Edit3,
  Pause,
  Play,
  Plus,
  RefreshCw,
  Trash2,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { ConfirmDialog, DataTable, type DataTableColumn } from '@/admin';
import * as syntheticsApi from '@/api/synthetics';
import type { MonitorKind } from '@/api/synthetics';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { ProductState } from '@/product/states';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { CheckEditor } from './CheckEditor';
import {
  KindLabel,
  StatePill,
  SyntheticsCanvas,
  SyntheticsFilterBar,
  SyntheticsListSurface,
  SyntheticsPage,
  WorkspaceBoundary,
} from './components';
import { useSyntheticsWorkspace, type CheckRow } from './data';
import {
  formatDuration,
  formatPercent,
  formatRelativeTimestamp,
  monitorTarget,
  scheduleText,
} from './model';

export function Checks({
  kinds,
  titleKey = 'checks.title',
  subtitleKey = 'checks.subtitle',
}: {
  kinds?: MonitorKind[];
  titleKey?: string;
  subtitleKey?: string;
}) {
  const { t, i18n } = useTranslation('synthetics');
  const navigate = useNavigate();
  const location = useLocation();
  const { monitorId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const canManage = hasPermission('synthetics.manage', access);
  const workspace = useSyntheticsWorkspace({ kinds });
  const query = searchParams.get('query') ?? '';
  const stateFilter = searchParams.get('state') ?? 'all';
  const kindFilter = searchParams.get('type') ?? 'all';
  const [cloneDetail, setCloneDetail] = React.useState<CheckRow['detail']>();
  const [deleteRow, setDeleteRow] = React.useState<CheckRow>();
  const isNew = location.pathname.endsWith('/new');
  const isEdit = location.pathname.endsWith('/edit');
  const detailQuery = useQuery({
    queryKey: ['synthetics', 'monitor', monitorId],
    queryFn: () => syntheticsApi.getMonitor(monitorId!),
    enabled: Boolean(monitorId && isEdit),
  });

  const refresh = () => queryClient.invalidateQueries({ queryKey: ['synthetics'] });
  const run = useMutation({
    mutationFn: syntheticsApi.runMonitor,
    onSuccess: () => toast.success(t('checks.run_queued')),
    onError: (error) => toast.error(toApiError(error).message),
  });
  const lifecycle = useMutation({
    mutationFn: ({ monitorId, paused }: { monitorId: string; paused: boolean }) =>
      paused ? syntheticsApi.resumeMonitor(monitorId) : syntheticsApi.pauseMonitor(monitorId),
    onSuccess: refresh,
    onError: (error) => toast.error(toApiError(error).message),
  });
  const archive = useMutation({
    mutationFn: syntheticsApi.archiveMonitor,
    onSuccess: async () => {
      setDeleteRow(undefined);
      await refresh();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const normalized = query.trim().toLowerCase();
  const rows = workspace.rows.filter((row) => {
    const text = [row.monitor.name, row.monitor.description, monitorTarget(row.revision?.spec), ...row.monitor.tags]
      .join(' ')
      .toLowerCase();
    return (
      (!normalized || text.includes(normalized)) &&
      (stateFilter === 'all' || row.monitor.state === stateFilter) &&
      (kindFilter === 'all' || row.monitor.kind === kindFilter)
    );
  });
  const closeEditor = () => {
    setCloneDetail(undefined);
    if (isNew || isEdit) navigate('/synthetics/checks', { replace: true });
  };
  const setFilter = (key: 'query' | 'state' | 'type', value: string) => {
    const next = new URLSearchParams(searchParams);
    if (!value || value === 'all') next.delete(key);
    else next.set(key, value);
    setSearchParams(next, { replace: true });
  };

  return (
    <SyntheticsPage
      title={t(titleKey)}
      subtitle={t(subtitleKey)}
      toolbar={
        <div className="flex items-center gap-2">
          <ChromeButton onClick={() => void workspace.refetch()} disabled={workspace.refetching}>
            <RefreshCw aria-hidden className={workspace.refetching ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
            {t('actions.refresh')}
          </ChromeButton>
          {canManage && (
            <ChromeButton variant="primary" onClick={() => navigate('/synthetics/checks/new')}>
              <Plus aria-hidden className="h-3.5 w-3.5" />
              {t('actions.create_check')}
            </ChromeButton>
          )}
        </div>
      }
      bodyClassName="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2"
    >
      <WorkspaceBoundary
        pending={workspace.pending}
        error={workspace.error}
        onRetry={() => void workspace.refetch()}
        flat
      >
        <SyntheticsCanvas>
          {workspace.rows.length === 0 ? (
            <ProductState
              variant="empty"
              title={t('states.empty_title')}
              description={t('states.empty_description')}
              className="rounded-none border-x-0 border-t-0 border-solid border-bd-0 bg-transparent"
              action={
                canManage ? (
                  <ChromeButton
                    variant="primary"
                    onClick={() => navigate('/synthetics/checks/new')}
                  >
                    <Plus className="h-3.5 w-3.5" />
                    {t('actions.create_first_check')}
                  </ChromeButton>
                ) : undefined
              }
            />
          ) : (
            <SyntheticsListSurface>
              <SyntheticsFilterBar className="grid gap-2 sm:grid-cols-[minmax(220px,1fr)_170px_170px]">
                <FormInput
                  aria-label={t('checks.search_placeholder')}
                  value={query}
                  onChange={(event) => setFilter('query', event.target.value)}
                  placeholder={t('checks.search_placeholder')}
                />
                <FormSelect
                  ariaLabel={t('checks.all_states')}
                  value={stateFilter}
                  onChange={(value) => setFilter('state', value)}
                  options={[
                    { value: 'all', label: t('checks.all_states') },
                    ...(['healthy', 'degraded', 'failing', 'unknown'] as const).map((state) => ({ value: state, label: t(`states.${state}`) })),
                  ]}
                />
                <FormSelect
                  ariaLabel={t('checks.all_types')}
                  value={kindFilter}
                  onChange={(value) => setFilter('type', value)}
                  options={[
                    { value: 'all', label: t('checks.all_types') },
                    ...(kinds ?? ALL_KINDS).map((kind) => ({ value: kind, label: t(`kinds.${kind}`) })),
                  ]}
                />
              </SyntheticsFilterBar>
              <div className="overflow-x-auto">
                <DataTable
                  rows={rows}
                  columns={columns({
                    t,
                    locale: i18n.language,
                    canManage,
                    onRun: (row) => run.mutate(row.monitor.id),
                    onEdit: (row) => navigate(`/synthetics/checks/${row.monitor.id}/edit`),
                    onClone: (row) => setCloneDetail(row.detail),
                    onToggle: (row) => lifecycle.mutate({ monitorId: row.monitor.id, paused: row.monitor.lifecycle === 'paused' }),
                    onDelete: setDeleteRow,
                  })}
                  rowKey={(row) => row.monitor.id}
                  onRowClick={(row) => navigate(`/synthetics/checks/${row.monitor.id}`)}
                  emptyLabel={t('states.filtered_empty')}
                  className="min-w-[1180px] rounded-none border-0 bg-transparent"
                />
              </div>
            </SyntheticsListSurface>
          )}
        </SyntheticsCanvas>
      </WorkspaceBoundary>

      <CheckEditor
        open={isNew || Boolean(cloneDetail) || Boolean(isEdit && detailQuery.data)}
        onOpenChange={(open) => !open && closeEditor()}
        mode={cloneDetail ? 'clone' : isEdit ? 'edit' : 'create'}
        detail={cloneDetail ?? detailQuery.data}
        locations={workspace.locations}
        onSaved={(id) => navigate(`/synthetics/checks/${id}`)}
        preset={searchParams.get('preset')}
      />
      <ConfirmDialog
        open={Boolean(deleteRow)}
        onOpenChange={(open) => !open && setDeleteRow(undefined)}
        title={t('actions.delete')}
        description={deleteRow ? t('checks.confirm_delete', { name: deleteRow.monitor.name }) : undefined}
        confirmLabel={t('actions.delete')}
        destructive
        busy={archive.isPending}
        onConfirm={() => deleteRow && archive.mutate(deleteRow.monitor.id)}
      />
    </SyntheticsPage>
  );
}

function columns({
  t,
  locale,
  canManage,
  onRun,
  onEdit,
  onClone,
  onToggle,
  onDelete,
}: {
  t: TFunction<'synthetics'>;
  locale: string;
  canManage: boolean;
  onRun: (row: CheckRow) => void;
  onEdit: (row: CheckRow) => void;
  onClone: (row: CheckRow) => void;
  onToggle: (row: CheckRow) => void;
  onDelete: (row: CheckRow) => void;
}): DataTableColumn<CheckRow>[] {
  return [
    { key: 'name', header: t('checks.columns.name'), width: 220, cell: (row) => <div className="min-w-0"><div className="truncate font-strong text-tx-0">{row.monitor.name}</div><div className="mt-0.5 truncate text-type-micro text-tx-3">{row.monitor.tags.join(' · ') || row.monitor.description}</div></div> },
    { key: 'type', header: t('checks.columns.type'), width: 120, cell: (row) => <KindLabel kind={row.monitor.kind} /> },
    { key: 'target', header: t('checks.columns.target'), width: 220, cell: (row) => <span className="font-code text-xs text-tx-2" title={monitorTarget(row.revision?.spec)}>{monitorTarget(row.revision?.spec)}</span> },
    { key: 'interval', header: t('checks.columns.interval'), width: 80, cell: (row) => scheduleText(row.revision?.schedule) },
    { key: 'locations', header: t('checks.columns.locations'), width: 85, cell: (row) => String(row.revision?.location_ids.length ?? 0) },
    { key: 'status', header: t('checks.columns.status'), width: 105, cell: (row) => <StatePill state={row.monitor.state} compact /> },
    { key: 'latency', header: t('checks.columns.latency'), width: 90, cell: (row) => formatDuration(row.latestLatencyMicros) },
    { key: 'success', header: t('checks.columns.success_rate'), width: 100, cell: (row) => formatPercent(row.successRate) },
    { key: 'last', header: t('checks.columns.last_run'), width: 115, cell: (row) => formatRelativeTimestamp(row.latestResult?.started_at, locale) },
    { key: 'enabled', header: t('checks.columns.enabled'), width: 75, cell: (row) => <button type="button" role="switch" aria-checked={row.monitor.lifecycle === 'active'} disabled={!canManage || !row.monitor.active_revision_id} onClick={(event) => { event.stopPropagation(); onToggle(row); }} className={`relative h-6 w-10 rounded-full transition-colors focus-visible:bg-indigo-soft ${row.monitor.lifecycle === 'active' ? 'bg-green' : 'bg-bg-4'} disabled:cursor-not-allowed disabled:opacity-50`}><span className={`absolute top-1 h-4 w-4 rounded-full bg-white transition-transform ${row.monitor.lifecycle === 'active' ? 'left-5' : 'left-1'}`} /></button> },
    { key: 'actions', header: <span className="sr-only">{t('checks.columns.actions')}</span>, width: 190, cell: (row) => <RowActions row={row} canManage={canManage} t={t} onRun={onRun} onEdit={onEdit} onClone={onClone} onToggle={onToggle} onDelete={onDelete} /> },
  ];
}

function RowActions({ row, canManage, t, onRun, onEdit, onClone, onToggle, onDelete }: { row: CheckRow; canManage: boolean; t: TFunction<'synthetics'>; onRun: (row: CheckRow) => void; onEdit: (row: CheckRow) => void; onClone: (row: CheckRow) => void; onToggle: (row: CheckRow) => void; onDelete: (row: CheckRow) => void }) {
  const actions = [
    [
      t('actions.run_now'),
      Play,
      onRun,
      !row.monitor.active_revision_id ||
        row.monitor.lifecycle !== 'active' ||
        row.monitor.kind === 'heartbeat',
    ],
    [t('actions.edit'), Edit3, onEdit, false],
    [t('actions.clone'), Copy, onClone, !row.detail],
    [row.monitor.lifecycle === 'paused' ? t('actions.resume') : t('actions.pause'), row.monitor.lifecycle === 'paused' ? Play : Pause, onToggle, !row.monitor.active_revision_id],
    [t('actions.delete'), Trash2, onDelete, false],
  ] as const;
  return <div className="flex items-center justify-end gap-0.5" onClick={(event) => event.stopPropagation()}>{actions.map(([label, Icon, action, disabled]) => <Tooltip key={label}><TooltipTrigger asChild><IconButton aria-label={label} disabled={!canManage || disabled} onClick={() => action(row)}><Icon className="h-3.5 w-3.5" /></IconButton></TooltipTrigger><TooltipContent>{label}</TooltipContent></Tooltip>)}</div>;
}

const ALL_KINDS: MonitorKind[] = ['http', 'browser', 'tcp', 'dns', 'icmp', 'tls', 'grpc', 'heartbeat'];
