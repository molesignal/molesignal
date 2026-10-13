import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { ConfirmDialog, DataTable } from '@/admin';
import * as syntheticsApi from '@/api/synthetics';
import type { MonitorState } from '@/api/synthetics';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { toast } from '@/shell/ui/sonner';
import { formatRelativeMicros } from '@/time/relative';

import { SyntheticsActivation } from '../Activation';
import { CheckEditor } from '../CheckEditor';
import { useSyntheticsWorkspace, type CheckRow } from '../data';
import { CreateCheckAction, SyntheticsPage, useLoadState } from '../Frame';
import { CheckStateMark, RunStrip } from '../marks';
import { checkTarget, formatDuration, monitorTarget, scheduleText } from '../model';
import { CHECK_FAMILIES, STATES_WORST_FIRST, checkFamily, type CheckFamily } from '../summary';
import { CheckRowActions } from './RowActions';

type FamilyView = CheckFamily | 'all';
type StateFilter = MonitorState | 'paused' | 'all';

const STATE_FILTERS: readonly StateFilter[] = [...STATES_WORST_FIRST, 'paused'];

/**
 * Every check: what it calls, how it stands, and how its latest runs went.
 *
 * The kinds of check are views of this one list, not tabs of the module, and a
 * check's schedule is a column of it. The strip of runs says at a glance what a
 * success rate would round away: one failure among twenty, or twenty in a row.
 */
export function Checks() {
  const { t, i18n } = useTranslation('synthetics');
  const navigate = useNavigate();
  const location = useLocation();
  const { monitorId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'synthetics.manage' });
  const workspace = useSyntheticsWorkspace();
  const [cloneDetail, setCloneDetail] = React.useState<CheckRow['detail']>();
  const [deleteRow, setDeleteRow] = React.useState<CheckRow>();
  const query = searchParams.get('query') ?? '';
  const family: FamilyView =
    CHECK_FAMILIES.find((item) => item === searchParams.get('kind')) ?? 'all';
  const stateFilter: StateFilter =
    STATE_FILTERS.find((item) => item === searchParams.get('state')) ?? 'all';
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
    mutationFn: ({ id, paused }: { id: string; paused: boolean }) =>
      paused ? syntheticsApi.resumeMonitor(id) : syntheticsApi.pauseMonitor(id),
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

  const state = useLoadState({
    pending: workspace.pending,
    error: workspace.error,
    onRetry: () => void workspace.refetch(),
  });
  const firstRun = !state && workspace.rows.length === 0;
  const needle = query.trim().toLocaleLowerCase();
  const rows = workspace.rows.filter((row) => {
    const text = [
      row.monitor.name,
      row.monitor.description,
      monitorTarget(row.revision?.spec),
      ...row.monitor.tags,
    ]
      .join(' ')
      .toLocaleLowerCase();

    return (
      (!needle || text.includes(needle)) &&
      (family === 'all' || checkFamily(row.monitor.kind) === family) &&
      (stateFilter === 'all' ||
        (stateFilter === 'paused'
          ? row.monitor.lifecycle === 'paused'
          : row.monitor.lifecycle === 'active' && row.monitor.state === stateFilter))
    );
  });
  const familyCount = (item: CheckFamily) =>
    workspace.rows.filter((row) => checkFamily(row.monitor.kind) === item).length;
  const setFilter = (key: 'query' | 'state' | 'kind', value: string) => {
    const next = new URLSearchParams(searchParams);
    if (!value || value === 'all') next.delete(key);
    else next.set(key, value);
    setSearchParams(next, { replace: true });
  };
  const closeEditor = () => {
    setCloneDetail(undefined);
    if (isNew || isEdit) navigate('/synthetics/checks', { replace: true });
  };
  const nowMicros = Date.now() * 1000;

  return (
    <>
      <SyntheticsPage
        // The first-run page carries the way to a first check itself.
        action={firstRun ? undefined : <CreateCheckAction />}
        state={state}
        {...(firstRun ? { contentSurface: false } : {})}
        listHead={
          firstRun ? undefined : (
            <>
              <ViewTabs<FamilyView>
                label={t('checks.views_label')}
                value={family}
                onValueChange={(next) => setFilter('kind', next)}
                items={[
                  { key: 'all', label: t('checks.views.all'), count: workspace.rows.length },
                  // A kind nobody has a check of is not a view worth offering.
                  ...CHECK_FAMILIES.filter((item) => item === family || familyCount(item) > 0).map(
                    (item) => ({
                      key: item,
                      label: t(`checks.views.${item}`),
                      count: familyCount(item),
                    }),
                  ),
                ]}
              />
              <label className="relative w-full min-w-0 sm:w-[280px]">
                <span className="sr-only">{t('checks.search_placeholder')}</span>
                <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
                <FormInput
                  value={query}
                  onChange={(event) => setFilter('query', event.target.value)}
                  placeholder={t('checks.search_placeholder')}
                  className="pl-9"
                />
              </label>
              <FormSelect
                ariaLabel={t('checks.state_filter')}
                className="w-full sm:w-[170px]"
                value={stateFilter}
                onChange={(value) => setFilter('state', value)}
                options={[
                  { value: 'all', label: t('checks.all_states') },
                  ...STATE_FILTERS.map((item) => ({
                    value: item,
                    label: t(`check_states.${item}`),
                  })),
                ]}
              />
            </>
          )
        }
      >
        {firstRun ? (
          <SyntheticsActivation />
        ) : (
          <div className="overflow-x-auto">
            <DataTable
              rows={rows}
              rowKey={(row) => row.monitor.id}
              onRowClick={(row) => navigate(`/synthetics/checks/${row.monitor.id}`)}
              emptyLabel={t('states.filtered_empty')}
              className="min-w-[940px] rounded-none border-0 bg-transparent"
              columns={[
                {
                  key: 'check',
                  header: t('checks.columns.check'),
                  cell: (row) => (
                    <div className="min-w-0 py-1">
                      <div className="truncate font-strong text-tx-0">{row.monitor.name}</div>
                      <div className="mt-0.5 flex min-w-0 items-baseline gap-[6px] text-xs text-tx-2">
                        <span className="shrink-0">{t(`kinds.${row.monitor.kind}`)}</span>
                        {/* A heartbeat calls nothing, and a journey has steps, not one target. */}
                        {checkTarget(row.revision?.spec) && (
                          <span
                            className="min-w-0 truncate font-code text-tx-3"
                            title={checkTarget(row.revision?.spec)}
                          >
                            {checkTarget(row.revision?.spec)}
                          </span>
                        )}
                      </div>
                    </div>
                  ),
                },
                {
                  key: 'status',
                  header: t('checks.columns.status'),
                  width: 120,
                  cell: (row) => (
                    <CheckStateMark state={row.monitor.state} lifecycle={row.monitor.lifecycle} />
                  ),
                },
                {
                  key: 'runs',
                  header: t('checks.columns.runs'),
                  width: 150,
                  cell: (row) => <RunStrip results={row.results} />,
                },
                {
                  key: 'latency',
                  header: t('checks.columns.latency'),
                  width: 100,
                  cell: (row) => (
                    <span className="tabular-nums text-tx-1">
                      {formatDuration(row.latestLatencyMicros)}
                    </span>
                  ),
                },
                {
                  key: 'schedule',
                  header: t('checks.columns.schedule'),
                  width: 170,
                  cell: (row) => (
                    <span className="text-tx-1">
                      {row.revision
                        ? row.revision.schedule.kind === 'heartbeat'
                          ? t('checks.schedule_heartbeat', {
                              interval: scheduleText(row.revision.schedule),
                            })
                          : t('checks.schedule', {
                              interval: scheduleText(row.revision.schedule),
                              count: row.revision.location_ids.length,
                            })
                        : '—'}
                    </span>
                  ),
                },
                {
                  key: 'last',
                  header: t('checks.columns.last_run'),
                  width: 130,
                  cell: (row) => (
                    <span className="text-tx-2">
                      {row.latestResult
                        ? formatRelativeMicros(row.latestResult.started_at, i18n.language, nowMicros)
                        : '—'}
                    </span>
                  ),
                },
                {
                  key: 'actions',
                  header: <span className="sr-only">{t('checks.columns.actions')}</span>,
                  width: 88,
                  className: 'overflow-visible text-right',
                  cell: (row) => (
                    <CheckRowActions
                      row={row}
                      access={manageAccess}
                      onRun={() => run.mutate(row.monitor.id)}
                      onEdit={() => navigate(`/synthetics/checks/${row.monitor.id}/edit`)}
                      onClone={() => setCloneDetail(row.detail)}
                      onToggle={() =>
                        lifecycle.mutate({
                          id: row.monitor.id,
                          paused: row.monitor.lifecycle === 'paused',
                        })
                      }
                      onDelete={() => setDeleteRow(row)}
                    />
                  ),
                },
              ]}
            />
          </div>
        )}
      </SyntheticsPage>

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
        description={
          deleteRow ? t('checks.confirm_delete', { name: deleteRow.monitor.name }) : undefined
        }
        confirmLabel={t('actions.delete')}
        destructive
        busy={archive.isPending}
        onConfirm={() => deleteRow && archive.mutate(deleteRow.monitor.id)}
      />
    </>
  );
}
