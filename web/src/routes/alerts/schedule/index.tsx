import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  CalendarDays,
  Copy,
  Ellipsis,
  Eye,
  PauseCircle,
  Pencil,
  PlayCircle,
  RefreshCw,
  Search,
  Settings2,
  Shuffle,
  Trash2,
  UserRoundPlus,
  UsersRound,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { ConfirmDialog, DataTable } from '@/admin';
import * as schedulesApi from '@/api/schedules';
import * as teamsApi from '@/api/teams';
import type { Team } from '@/api/teams';
import { toApiError } from '@/lib/http';
import { type ActionAccess, useActionAccess } from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { queryStateFor } from '@/shell/query/State';
import { ResultPagination } from '@/shell/ResultPagination';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { useUsers, type UserLite } from '@/shell/useUsers';
import { useAuthStore } from '@/stores/auth';
import type {
  Rotation,
  Schedule,
} from '@/types/alerting';

import { ScheduleEditorDrawer } from './EditorDrawer';
export {
  localInputToMicros,
  microsToLocalInput,
} from './EditorDrawer';
import {
  MICROS_PER_DAY,
  liveOrFutureOverrides,
  nextScheduleBoundary,
  resolveScheduleAt,
  rotationKindKey,
  rotationRole,
  scheduleMemberIds,
  scheduleStatus,
  type ScheduleStatus,
  timezoneDisplay,
} from './model';
import {
  ScheduleSummaryCard,
  UserAvatar,
  formatScheduleDateTime,
  formatScheduleTime,
  relativeDuration,
} from './Ui';
import { AlertsSubNav } from '../Layout';

interface ScheduleListRow {
  schedule: Schedule;
  team: Team | null;
  currentUser: UserLite | null;
  memberIds: string[];
  status: ScheduleStatus;
  nextSwitch: number | null;
  futureOverrideCount: number;
  updatedBy: UserLite | null;
}

type StatusFilter = 'all' | ScheduleStatus;

export function AlertsSchedules() {
  const { t, i18n } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const users = useUsers();
  const manageAccess = useActionAccess({ permission: 'schedules.manage' });
  const currentUserId = useAuthStore(
    (state) => state.ctx?.user_id ?? '',
  );
  const [nowMicros, setNowMicros] = React.useState(
    () => Date.now() * 1000,
  );
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<Schedule | null>(null);
  const [removing, setRemoving] = React.useState<Schedule | null>(null);
  const [query, setQuery] = React.useState('');
  const [teamFilter, setTeamFilter] = React.useState('all');
  const [timezoneFilter, setTimezoneFilter] = React.useState('all');
  const [statusFilter, setStatusFilter] =
    React.useState<StatusFilter>('all');
  const [mineOnly, setMineOnly] = React.useState(false);
  const [overrideOnly, setOverrideOnly] = React.useState(false);
  const [showRecentChange, setShowRecentChange] =
    React.useState(true);
  const [page, setPage] = React.useState(1);
  const [pageSize, setPageSize] = React.useState(10);

  React.useEffect(() => {
    const timer = window.setInterval(
      () => setNowMicros(Date.now() * 1000),
      60_000,
    );
    return () => window.clearInterval(timer);
  }, []);

  const schedulesQuery = useQuery({
    queryKey: ['schedules'],
    queryFn: schedulesApi.list,
  });
  const teamsQuery = useQuery({
    queryKey: ['teams'],
    queryFn: teamsApi.list,
  });
  const schedules = React.useMemo(
    () => schedulesQuery.data ?? [],
    [schedulesQuery.data],
  );
  const teams = React.useMemo(
    () => teamsQuery.data ?? [],
    [teamsQuery.data],
  );
  const teamsById = React.useMemo(
    () => new Map(teams.map((team) => [team.id, team])),
    [teams],
  );

  const rows = React.useMemo<ScheduleListRow[]>(
    () =>
      schedules.map((schedule) => {
        const current = resolveScheduleAt(schedule, nowMicros);
        return {
          schedule,
          team: schedule.team_id
            ? teamsById.get(schedule.team_id) ?? null
            : null,
          currentUser: current?.userId
            ? users.byId.get(current.userId) ?? null
            : null,
          memberIds: scheduleMemberIds(schedule),
          status: scheduleStatus(schedule, nowMicros),
          nextSwitch: nextScheduleBoundary(schedule, nowMicros),
          futureOverrideCount: liveOrFutureOverrides(
            schedule,
            nowMicros,
          ).length,
          updatedBy: schedule.updated_by
            ? users.byId.get(schedule.updated_by) ?? null
            : null,
        };
      }),
    [nowMicros, schedules, teamsById, users.byId],
  );

  const filteredRows = React.useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return rows.filter((row) => {
      const searchable = [
        row.schedule.name,
        row.schedule.description,
        row.team?.name ?? '',
        ...row.memberIds.map(
          (id) => users.byId.get(id)?.name ?? id,
        ),
      ]
        .join(' ')
        .toLocaleLowerCase();
      return (
        (!normalized || searchable.includes(normalized))
        && (teamFilter === 'all'
          || row.schedule.team_id === teamFilter)
        && (timezoneFilter === 'all'
          || row.schedule.timezone === timezoneFilter)
        && (statusFilter === 'all'
          || row.status === statusFilter
          || (statusFilter === 'active'
            && row.status === 'switching'))
        && (!mineOnly || row.memberIds.includes(currentUserId))
        && (!overrideOnly || row.futureOverrideCount > 0)
      );
    });
  }, [
    currentUserId,
    mineOnly,
    overrideOnly,
    query,
    rows,
    statusFilter,
    teamFilter,
    timezoneFilter,
    users.byId,
  ]);

  React.useEffect(() => setPage(1), [
    query,
    teamFilter,
    timezoneFilter,
    statusFilter,
    mineOnly,
    overrideOnly,
    pageSize,
  ]);

  const pageCount = Math.max(
    1,
    Math.ceil(filteredRows.length / pageSize),
  );
  const pagedRows = filteredRows.slice(
    (Math.min(page, pageCount) - 1) * pageSize,
    Math.min(page, pageCount) * pageSize,
  );

  const state = queryStateFor({
    isLoading: schedulesQuery.isLoading,
    isError: schedulesQuery.isError,
    data: schedules,
  });
  const pageState = productStateFor(state, {
    error: schedulesQuery.error,
    emptyTitle: t('schedules.empty_title'),
    emptyDescription: t('schedules.empty_description'),
  });

  const remove = useMutation({
    mutationFn: schedulesApi.remove,
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void queryClient.invalidateQueries({ queryKey: ['schedules'] });
      setRemoving(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const toggle = useMutation({
    mutationFn: (schedule: Schedule) =>
      schedulesApi.update(schedule.id, {
        ...scheduleInput(schedule),
        enabled: !schedule.enabled,
      }),
    onSuccess: (schedule) => {
      toast.success(
        schedule.enabled
          ? t('schedules.toast_resumed')
          : t('schedules.toast_paused'),
      );
      void queryClient.invalidateQueries({ queryKey: ['schedules'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const copy = useMutation({
    mutationFn: (schedule: Schedule) =>
      schedulesApi.create({
        ...scheduleInput(schedule),
        name: t('schedules.copy_name', { name: schedule.name }),
        enabled: false,
        overrides: [],
      }),
    onSuccess: (schedule) => {
      toast.success(t('schedules.toast_copied'));
      void queryClient.invalidateQueries({ queryKey: ['schedules'] });
      navigate(`/alerts/schedules/${schedule.id}`);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const summary = React.useMemo(() => {
    const activeRows = rows.filter(
      (row) =>
        row.status === 'active' || row.status === 'switching',
    );
    const activeUsers = new Set(
      activeRows
        .map((row) => row.currentUser?.id)
        .filter(Boolean),
    );
    const overrideRows = rows.filter(
      (row) => row.futureOverrideCount > 0,
    );
    const overrideUsers = new Set(
      schedules.flatMap((schedule) =>
        liveOrFutureOverrides(schedule, nowMicros).map(
          (override) => override.user_id,
        ),
      ),
    );
    const teamCount = new Set(
      schedules.map((schedule) => schedule.team_id).filter(Boolean),
    ).size;
    const weekEnd = nowMicros + 7 * MICROS_PER_DAY;
    let weeklyRotations = 0;
    let nearestSwitch: number | null = null;
    for (const row of rows) {
      let cursor = nowMicros;
      for (let index = 0; index < 16; index += 1) {
        const next = nextScheduleBoundary(
          row.schedule,
          cursor,
          weekEnd - cursor,
        );
        if (!next || next > weekEnd) break;
        weeklyRotations += 1;
        nearestSwitch =
          nearestSwitch === null
            ? next
            : Math.min(nearestSwitch, next);
        cursor = next;
      }
    }
    return {
      activeRows,
      activeUsers,
      overrideRows,
      overrideUsers,
      teamCount,
      weeklyRotations,
      nearestSwitch,
    };
  }, [nowMicros, rows, schedules]);

  const timezones = React.useMemo(
    () =>
      Array.from(new Set(schedules.map((schedule) => schedule.timezone)))
        .filter(Boolean)
        .sort(),
    [schedules],
  );

  return (
    <>
      <PageHeader
        title={t('schedules.title')}
        subtitle={t('schedules.subtitle')}
        toolbar={
          <ChromeButton
            variant="primary"
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onClick={() => setCreating(true)}
          >
            {t('schedules.new_schedule')}
          </ChromeButton>
        }
      />
      <AlertsSubNav />

      <PageBody className="overflow-auto">
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <div className="flex min-w-0 flex-col gap-4">
            <div className="grid grid-cols-2 gap-3 xl:grid-cols-4">
              <ScheduleSummaryCard
                icon={CalendarDays}
                label={t('schedules.summary.schedules')}
                value={t('schedules.summary.schedule_count', {
                  count: schedules.length,
                })}
                hint={t('schedules.summary.team_count', {
                  count: summary.teamCount,
                })}
                tone="indigo"
              />
              <ScheduleSummaryCard
                icon={UsersRound}
                label={t('schedules.summary.on_call')}
                value={t('schedules.summary.active_count', {
                  count: summary.activeRows.length,
                })}
                hint={t('schedules.summary.people_on_call', {
                  count: summary.activeUsers.size,
                })}
                tone="green"
                onClick={() => {
                  setStatusFilter('active');
                  setOverrideOnly(false);
                }}
              />
              <ScheduleSummaryCard
                icon={RefreshCw}
                label={t('schedules.summary.this_week')}
                value={t('schedules.summary.rotation_count', {
                  count: summary.weeklyRotations,
                })}
                hint={
                  summary.nearestSwitch
                    ? t('schedules.summary.next_switch', {
                        value: formatScheduleDateTime(
                          summary.nearestSwitch,
                          i18n.language,
                        ),
                      })
                    : t('schedules.summary.no_switch')
                }
                tone="blue"
              />
              <ScheduleSummaryCard
                icon={Shuffle}
                label={t('schedules.summary.overrides')}
                value={t('schedules.summary.override_count', {
                  count: summary.overrideRows.length,
                })}
                hint={t('schedules.summary.override_people', {
                  count: summary.overrideUsers.size,
                })}
                tone="orange"
                onClick={() => {
                  setOverrideOnly(true);
                  setStatusFilter('all');
                }}
              />
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <label className="relative min-w-[280px] flex-1">
                <span className="sr-only">
                  {t('schedules.filters.search')}
                </span>
                <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-tx-3" />
                <input
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  placeholder={t('schedules.filters.search')}
                  className="h-9 w-full rounded-md border border-bd-1 bg-bg-1 pl-9 pr-3 text-sm text-tx-0 outline-none placeholder:text-tx-3 focus:border-indigo/60"
                />
              </label>
              <FilterSelect
                label={t('schedules.filters.team')}
                value={teamFilter}
                onChange={setTeamFilter}
                options={[
                  {
                    value: 'all',
                    label: t('schedules.filters.all'),
                  },
                  ...teams.map((team) => ({
                    value: team.id,
                    label: team.name,
                  })),
                ]}
              />
              <FilterSelect
                label={t('schedules.filters.timezone')}
                value={timezoneFilter}
                onChange={setTimezoneFilter}
                options={[
                  {
                    value: 'all',
                    label: t('schedules.filters.all'),
                  },
                  ...timezones.map((timezone) => ({
                    value: timezone,
                    label: timezone,
                  })),
                ]}
              />
              <FilterSelect
                label={t('schedules.filters.status')}
                value={statusFilter}
                onChange={(value) =>
                  setStatusFilter(value as StatusFilter)
                }
                options={[
                  {
                    value: 'all',
                    label: t('schedules.filters.all'),
                  },
                  ...(
                    [
                      'active',
                      'switching',
                      'not_started',
                      'paused',
                      'gap',
                    ] as const
                  ).map((status) => ({
                    value: status,
                    label: t(`schedules.status.${status}`),
                  })),
                ]}
              />
              <label className="flex h-9 items-center gap-2 rounded-md border border-bd-0 bg-bg-1 px-3 text-xs font-strong text-tx-1">
                {t('schedules.filters.mine')}
                <Switch
                  checked={mineOnly}
                  onCheckedChange={setMineOnly}
                />
              </label>
              {(overrideOnly
                || query
                || teamFilter !== 'all'
                || timezoneFilter !== 'all'
                || statusFilter !== 'all'
                || mineOnly) && (
                <ChromeButton
                  size="sm"
                  onClick={() => {
                    setQuery('');
                    setTeamFilter('all');
                    setTimezoneFilter('all');
                    setStatusFilter('all');
                    setMineOnly(false);
                    setOverrideOnly(false);
                  }}
                >
                  {t('schedules.filters.reset')}
                </ChromeButton>
              )}
              <ChromeButton
                size="sm"
                className="ml-auto px-2"
                aria-label={t('schedules.filters.refresh')}
                title={t('schedules.filters.refresh')}
                onClick={() => {
                  setNowMicros(Date.now() * 1000);
                  void schedulesQuery.refetch();
                }}
              >
                <RefreshCw className="h-4 w-4" />
              </ChromeButton>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <ChromeButton
                    size="sm"
                    className="px-2"
                    aria-label={t('schedules.filters.list_settings')}
                    title={t('schedules.filters.list_settings')}
                  >
                    <Settings2 className="h-4 w-4" />
                  </ChromeButton>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuCheckboxItem
                    checked={showRecentChange}
                    onCheckedChange={(checked) =>
                      setShowRecentChange(checked === true)
                    }
                  >
                    {t('schedules.columns.recent_change')}
                  </DropdownMenuCheckboxItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>

            <div className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
              <DataTable
                rows={pagedRows}
                rowKey={(row) => row.schedule.id}
                onRowClick={(row) =>
                  navigate(`/alerts/schedules/${row.schedule.id}`)
                }
                emptyLabel={t('schedules.filters.no_results')}
                columns={[
                  {
                    key: 'name',
                    header: t('schedules.columns.name'),
                    width: '23%',
                    cell: (row) => (
                      <ScheduleNameCell
                        row={row}
                        showTeam={true}
                      />
                    ),
                  },
                  {
                    key: 'current',
                    header: t('schedules.columns.current_on_call'),
                    width: 190,
                    cell: (row) => (
                      <CurrentOnCallCell
                        row={row}
                        emptyLabel={t('schedules.nobody_on_call')}
                        activeLabel={t(
                          'schedules.currently_on_call',
                        )}
                      />
                    ),
                  },
                  {
                    key: 'timezone',
                    header: t('schedules.columns.timezone'),
                    width: 130,
                    cell: (row) => {
                      const display = timezoneDisplay(
                        row.schedule.timezone,
                      );
                      return (
                        <div>
                          <div className="font-mono text-xs font-strong text-tx-1">
                            {display.technical}
                          </div>
                          <div className="mt-0.5 truncate text-type-micro text-tx-3">
                            {i18n.language.startsWith('zh')
                              ? display.label
                              : row.schedule.timezone}
                          </div>
                        </div>
                      );
                    },
                  },
                  {
                    key: 'rotation',
                    header: t('schedules.columns.rotation'),
                    width: 150,
                    cell: (row) => (
                      <RotationCell
                        rotation={row.schedule.rotations[0]}
                        memberCount={row.memberIds.length}
                      />
                    ),
                  },
                  {
                    key: 'next',
                    header: t('schedules.columns.next_switch'),
                    width: 160,
                    cell: (row) => (
                      <NextSwitchCell
                        at={row.nextSwitch}
                        now={nowMicros}
                        locale={i18n.language}
                        timeZone={row.schedule.timezone}
                      />
                    ),
                  },
                  {
                    key: 'coverage',
                    header: t('schedules.columns.coverage'),
                    width: 110,
                    cell: (row) => (
                      <div>
                        <div className="text-xs font-strong text-tx-1">
                          {t('schedules.people_count', {
                            count: row.memberIds.length,
                          })}
                        </div>
                        <div className="mt-0.5 text-type-micro text-tx-3">
                          {t('schedules.rotation_count', {
                            count: row.schedule.rotations.length,
                          })}
                        </div>
                      </div>
                    ),
                  },
                  ...(showRecentChange
                    ? [
                        {
                          key: 'recent',
                          header: t(
                            'schedules.columns.recent_change',
                          ),
                          width: 170,
                          cell: (row: ScheduleListRow) => (
                            <div className="flex items-center gap-2">
                              <UserAvatar
                                user={row.updatedBy}
                                size="sm"
                                muted={!row.updatedBy}
                              />
                              <div className="min-w-0">
                                <div className="truncate text-xs font-strong text-tx-1">
                                  {row.updatedBy?.name
                                    ?? t('schedules.system_actor')}
                                </div>
                                <div className="mt-0.5 font-mono text-type-micro text-tx-3">
                                  {formatScheduleDateTime(
                                    row.schedule.updated_at,
                                    i18n.language,
                                  )}
                                </div>
                              </div>
                            </div>
                          ),
                        },
                      ]
                    : []),
                  {
                    key: 'status',
                    header: t('schedules.columns.status'),
                    width: 100,
                    cell: (row) => (
                      <StatusPill status={row.status} />
                    ),
                  },
                  {
                    key: 'actions',
                    header: '',
                    width: 56,
                    className: 'overflow-visible text-right',
                    cell: (row) => (
                      <ScheduleActions
                        row={row}
                        busy={
                          toggle.isPending
                          || copy.isPending
                        }
                        manageAccess={manageAccess}
                        onView={() =>
                          navigate(
                            `/alerts/schedules/${row.schedule.id}`,
                          )
                        }
                        onEdit={() => setEditing(row.schedule)}
                        onOverride={() =>
                          navigate(
                            `/alerts/schedules/${row.schedule.id}?addOverride=1`,
                          )
                        }
                        onToggle={() =>
                          toggle.mutate(row.schedule)
                        }
                        onCopy={() => copy.mutate(row.schedule)}
                        onDelete={() => setRemoving(row.schedule)}
                      />
                    ),
                  },
                ]}
              />
              <div className="flex items-center border-t border-bd-0 bg-bg-1">
                <span className="px-3 text-xs text-tx-3">
                  {t('schedules.total_count', {
                    count: filteredRows.length,
                  })}
                </span>
                <ResultPagination
                  page={Math.min(page, pageCount)}
                  pageCount={pageCount}
                  pageSize={pageSize}
                  pageSizeOptions={[10, 20, 50]}
                  pageLabel={t('schedules.pagination.page', {
                    page: Math.min(page, pageCount),
                    count: pageCount,
                  })}
                  ariaLabel={t('schedules.pagination.label')}
                  pageSizeAriaLabel={t(
                    'schedules.pagination.page_size',
                  )}
                  firstAriaLabel={t('schedules.pagination.first')}
                  previousAriaLabel={t(
                    'schedules.pagination.previous',
                  )}
                  nextAriaLabel={t('schedules.pagination.next')}
                  lastAriaLabel={t('schedules.pagination.last')}
                  onPageChange={setPage}
                  onPageSizeChange={setPageSize}
                  className="ml-auto border-t-0"
                />
              </div>
            </div>
          </div>
        )}
      </PageBody>

      <ScheduleEditorDrawer
        open={creating || editing !== null}
        editing={editing}
        users={users.users}
        teams={teams}
        onClose={() => {
          setCreating(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('schedules.delete_title')}
        description={removing?.name ?? ''}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) remove.mutate(removing.id);
        }}
      />
    </>
  );
}

function FilterSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (value: string) => void;
}) {
  return (
    <label className="flex h-9 min-w-36 items-center gap-2 rounded-md border border-bd-1 bg-bg-1 px-3">
      <span className="shrink-0 text-xs text-tx-3">{label}</span>
      <select
        value={value}
        aria-label={label}
        onChange={(event) => onChange(event.target.value)}
        className="min-w-0 flex-1 bg-transparent text-xs font-strong text-tx-1 outline-none"
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}

function ScheduleNameCell({
  row,
  showTeam,
}: {
  row: ScheduleListRow;
  showTeam: boolean;
}) {
  const { t } = useTranslation('alerts');
  const role = rotationRole(row.schedule);
  return (
    <div className="flex min-w-0 items-center gap-3">
      <span className="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-indigo-dim text-indigo-soft">
        <CalendarDays className="h-4.5 w-4.5" />
      </span>
      <span className="min-w-0">
        <span className="flex min-w-0 items-center gap-2">
          <span className="truncate text-sm font-display text-tx-0">
            {row.schedule.name}
          </span>
          <Pill tone="blue">
            {t(`schedules.roles.${role}`)}
          </Pill>
        </span>
        <span className="mt-0.5 block truncate text-type-micro text-tx-3">
          {row.schedule.description
            || t('schedules.description_fallback')}
        </span>
        {showTeam && (
          <span className="mt-0.5 block truncate text-type-micro text-tx-3">
            {t('schedules.team_prefix', {
              team:
                row.team?.name
                ?? t('schedules.team_unassigned'),
            })}
          </span>
        )}
      </span>
    </div>
  );
}

function CurrentOnCallCell({
  row,
  emptyLabel,
  activeLabel,
}: {
  row: ScheduleListRow;
  emptyLabel: string;
  activeLabel: string;
}) {
  return (
    <div className="flex items-center gap-2.5">
      <UserAvatar
        user={row.currentUser}
        online={Boolean(row.currentUser)}
        muted={!row.currentUser}
      />
      <div className="min-w-0">
        <div className="truncate text-xs font-strong text-tx-0">
          {row.currentUser?.name ?? emptyLabel}
        </div>
        <div
          className={
            row.currentUser
              ? 'mt-0.5 text-type-micro text-green-soft'
              : 'mt-0.5 text-type-micro text-tx-3'
          }
        >
          {row.currentUser
            ? activeLabel
            : '—'}
        </div>
      </div>
    </div>
  );
}

function RotationCell({
  rotation,
  memberCount,
}: {
  rotation: Rotation | undefined;
  memberCount: number;
}) {
  const { t } = useTranslation('alerts');
  if (!rotation) return <span className="text-tx-3">—</span>;
  return (
    <div>
      <div className="flex items-center gap-1.5">
        <span className="truncate text-xs font-strong text-tx-1">
          {rotation.name || t('schedules.rotation_untitled', { n: 1 })}
        </span>
        <Pill tone="blue">
          {t(
            `schedules.rotation_kinds.${rotationKindKey(rotation)}`,
          )}
        </Pill>
      </div>
      <div className="mt-0.5 text-type-micro text-tx-3">
        {t('schedules.people_rotation', {
          count: memberCount,
        })}
      </div>
    </div>
  );
}

function NextSwitchCell({
  at,
  now,
  locale,
  timeZone,
}: {
  at: number | null;
  now: number;
  locale: string;
  timeZone: string;
}) {
  const { t } = useTranslation('alerts');
  if (!at) {
    return (
      <div>
        <div className="text-xs text-tx-3">
          {t('schedules.no_switch')}
        </div>
        <div className="mt-0.5 text-type-micro text-tx-3">—</div>
      </div>
    );
  }
  return (
    <div>
      <div className="text-xs font-strong text-tx-1">
        {formatScheduleTime(at, locale, timeZone)}
      </div>
      <div className="mt-0.5 text-type-micro text-tx-3">
        {relativeDuration(at, now, locale)}
      </div>
    </div>
  );
}

function StatusPill({ status }: { status: ScheduleStatus }) {
  const { t } = useTranslation('alerts');
  const tone = {
    active: 'green',
    switching: 'orange',
    not_started: 'dim',
    paused: 'dim',
    gap: 'red',
  }[status] as 'green' | 'orange' | 'dim' | 'red';
  return <Pill tone={tone}>{t(`schedules.status.${status}`)}</Pill>;
}

function ScheduleActions({
  row,
  busy,
  onView,
  onEdit,
  onOverride,
  onToggle,
  onCopy,
  onDelete,
  manageAccess,
}: {
  row: ScheduleListRow;
  busy: boolean;
  onView: () => void;
  onEdit: () => void;
  onOverride: () => void;
  onToggle: () => void;
  onCopy: () => void;
  onDelete: () => void;
  manageAccess: ActionAccess;
}) {
  const { t } = useTranslation('alerts');
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label={t('schedules.actions.open_menu', {
            name: row.schedule.name,
          })}
          onClick={(event) => event.stopPropagation()}
          className="grid h-8 w-8 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0"
        >
          <Ellipsis className="h-4 w-4" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-48">
        <DropdownMenuItem onSelect={onView}>
          <Eye className="h-4 w-4" />
          {t('schedules.actions.view')}
        </DropdownMenuItem>
        <DropdownMenuItem
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onSelect={onEdit}
        >
          <Pencil className="h-4 w-4" />
          {t('schedules.actions.edit')}
        </DropdownMenuItem>
        <DropdownMenuItem
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onSelect={onOverride}
        >
          <UserRoundPlus className="h-4 w-4" />
          {t('schedules.actions.add_override')}
        </DropdownMenuItem>
        <DropdownMenuItem
          disabled={busy || manageAccess.disabled}
          disabledReason={!busy ? manageAccess.reason : undefined}
          onSelect={onToggle}
        >
          {row.schedule.enabled ? (
            <PauseCircle className="h-4 w-4" />
          ) : (
            <PlayCircle className="h-4 w-4" />
          )}
          {row.schedule.enabled
            ? t('schedules.actions.pause')
            : t('schedules.actions.resume')}
        </DropdownMenuItem>
        <DropdownMenuItem
          disabled={busy || manageAccess.disabled}
          disabledReason={!busy ? manageAccess.reason : undefined}
          onSelect={onCopy}
        >
          <Copy className="h-4 w-4" />
          {t('schedules.actions.copy')}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          className="text-red-soft focus:text-red-soft"
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onSelect={onDelete}
        >
          <Trash2 className="h-4 w-4" />
          {t('schedules.actions.delete')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function scheduleInput(
  schedule: Schedule,
): schedulesApi.ScheduleInput {
  return {
    name: schedule.name,
    description: schedule.description,
    team_id: schedule.team_id ?? null,
    timezone: schedule.timezone,
    enabled: schedule.enabled,
    rotations: schedule.rotations,
    overrides: schedule.overrides,
  };
}
