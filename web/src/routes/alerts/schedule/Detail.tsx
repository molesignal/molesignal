import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  CalendarClock,
  CalendarDays,
  Copy,
  Download,
  Ellipsis,
  PauseCircle,
  Pencil,
  PlayCircle,
  Trash2,
  UserRoundPlus,
  UsersRound,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import {
  useNavigate,
  useParams,
  useSearchParams,
} from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as auditApi from '@/api/audit';
import type { AuditEvent } from '@/api/audit';
import * as schedulesApi from '@/api/schedules';
import * as teamsApi from '@/api/teams';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, Dot, Pill } from '@/shell/chrome';
import { DateTimePicker } from '@/shell/DateTimePicker';
import { ErrorState } from '@/shell/ErrorState';
import {
  FormField,
  FormInput,
  FormRow,
  FormSelect,
} from '@/shell/FormDrawer';
import { LoadingState } from '@/shell/LoadingState';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { useUsers, type UserLite } from '@/shell/useUsers';
import type {
  Rotation,
  Schedule,
  ScheduleOverride,
} from '@/types/alerting';

import { ScheduleEditorDrawer } from './EditorDrawer';
import {
  localInputToMicros,
  microsToLocalInput,
} from './EditorDrawer';
import {
  buildScheduleTimeline,
  liveOrFutureOverrides,
  nextScheduleBoundary,
  resolveScheduleAt,
  resolutionStartedAt,
  rotationKindKey,
  scheduleMemberIds,
  scheduleStatus,
  timezoneDisplay,
} from './model';
import {
  ScheduleCard,
  ScheduleSummaryCard,
  UserAvatar,
  formatScheduleDateTime,
  formatScheduleDay,
  formatScheduleTime,
  relativeDuration,
} from './Ui';

interface ActivityRow {
  id: string;
  at: number;
  actorId: string | null;
  action: string;
  payload: Record<string, unknown>;
}

export function AlertsScheduleDetail() {
  const { id = '' } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const { t, i18n } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const users = useUsers();
  const manageAccess = useActionAccess({ permission: 'schedules.manage' });
  const [nowMicros, setNowMicros] = React.useState(
    () => Date.now() * 1000,
  );
  const [editOpen, setEditOpen] = React.useState(false);
  const [removingSchedule, setRemovingSchedule] =
    React.useState(false);
  const [editingOverride, setEditingOverride] =
    React.useState<ScheduleOverride | null>(null);
  const [removingOverride, setRemovingOverride] =
    React.useState<ScheduleOverride | null>(null);
  const [showOverrideHistory, setShowOverrideHistory] =
    React.useState(false);
  const [showAllActivity, setShowAllActivity] =
    React.useState(false);

  React.useEffect(() => {
    const timer = window.setInterval(
      () => setNowMicros(Date.now() * 1000),
      60_000,
    );
    return () => window.clearInterval(timer);
  }, []);

  const scheduleQuery = useQuery({
    queryKey: ['schedules', id],
    queryFn: () => schedulesApi.get(id),
    enabled: Boolean(id),
  });
  const schedulesQuery = useQuery({
    queryKey: ['schedules'],
    queryFn: schedulesApi.list,
  });
  const teamsQuery = useQuery({
    queryKey: ['teams'],
    queryFn: teamsApi.list,
  });
  const auditQuery = useQuery({
    queryKey: ['schedule-activity', id],
    queryFn: () =>
      auditApi.query({
        target_kind: 'schedule',
        target_id: id,
        limit: 50,
      }),
    enabled: Boolean(id),
    retry: false,
  });
  const schedule = scheduleQuery.data;

  const invalidate = React.useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: ['schedules'] });
    void queryClient.invalidateQueries({
      queryKey: ['schedule-activity', id],
    });
  }, [id, queryClient]);

  const toggle = useMutation({
    mutationFn: (current: Schedule) =>
      schedulesApi.update(current.id, {
        ...scheduleInput(current),
        enabled: !current.enabled,
      }),
    onSuccess: (saved) => {
      toast.success(
        saved.enabled
          ? t('schedules.toast_resumed')
          : t('schedules.toast_paused'),
      );
      invalidate();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const copy = useMutation({
    mutationFn: (current: Schedule) =>
      schedulesApi.create({
        ...scheduleInput(current),
        name: t('schedules.copy_name', { name: current.name }),
        enabled: false,
        overrides: [],
      }),
    onSuccess: (saved) => {
      toast.success(t('schedules.toast_copied'));
      invalidate();
      navigate(`/alerts/schedules/${saved.id}`);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const removeSchedule = useMutation({
    mutationFn: () => schedulesApi.remove(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void queryClient.invalidateQueries({ queryKey: ['schedules'] });
      navigate('/alerts/schedules');
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const removeOverride = useMutation({
    mutationFn: (overrideId: string) =>
      schedulesApi.removeOverride(id, overrideId),
    onSuccess: () => {
      toast.success(t('schedules.override_deleted'));
      setRemovingOverride(null);
      invalidate();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const endOverride = useMutation({
    mutationFn: (override: ScheduleOverride) =>
      schedulesApi.updateOverride(id, override.id, {
        user_id: override.user_id,
        start_at_micros: override.start_at,
        end_at_micros: Math.max(
          override.start_at + 1_000_000,
          Date.now() * 1000,
        ),
        reason: override.reason,
      }),
    onSuccess: () => {
      toast.success(t('schedules.override_ended'));
      invalidate();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (scheduleQuery.isLoading) {
    return (
      <>
        <PageHeader
          title={t('schedules.detail_loading')}
          backTo="/alerts/schedules"
        />
        <PageBody>
          <LoadingState variant="list" rows={7} />
        </PageBody>
      </>
    );
  }

  if (scheduleQuery.isError || !schedule) {
    return (
      <>
        <PageHeader
          title={t('schedules.detail_loading')}
          backTo="/alerts/schedules"
        />
        <PageBody>
          <ErrorState
            error={scheduleQuery.error}
            title={t('schedules.detail_error')}
            onRetry={() => void scheduleQuery.refetch()}
          />
        </PageBody>
      </>
    );
  }

  const currentResolution = resolveScheduleAt(
    schedule,
    nowMicros,
  );
  const currentUser = currentResolution
    ? users.byId.get(currentResolution.userId) ?? null
    : null;
  const currentStartedAt = resolutionStartedAt(
    schedule,
    currentResolution,
    nowMicros,
  );
  const nextSwitch = nextScheduleBoundary(schedule, nowMicros);
  const memberIds = scheduleMemberIds(schedule);
  const upcomingOverrides = liveOrFutureOverrides(
    schedule,
    nowMicros,
  );
  const activeOverrides = upcomingOverrides.filter(
    (override) =>
      override.start_at <= nowMicros && nowMicros < override.end_at,
  );
  const primaryRotation = schedule.rotations[0];
  const status = scheduleStatus(schedule, nowMicros);
  const timeZone = timezoneDisplay(schedule.timezone);
  const timeline = buildScheduleTimeline(schedule, nowMicros, 7).slice(
    0,
    7,
  );
  const activity = buildActivityRows(schedule, auditQuery.data?.items);
  const visibleActivity = showAllActivity
    ? activity
    : activity.slice(0, 5);
  const visibleOverrides = showOverrideHistory
    ? [...schedule.overrides].sort((a, b) => b.start_at - a.start_at)
    : upcomingOverrides;
  const addOverrideRequested =
    searchParams.get('addOverride') === '1' && manageAccess.allowed;

  return (
    <>
      <PageHeader
        title={
          <span className="flex min-w-0 flex-wrap items-center gap-2">
            <span className="truncate">{schedule.name}</span>
            <Pill tone="dim">{timeZone.technical}</Pill>
            <ScheduleStatusPill status={status} />
          </span>
        }
        subtitle={
          schedule.description || t('schedules.description_fallback')
        }
        backTo="/alerts/schedules"
        toolbar={
          <>
            <ChromeButton
              variant="primary"
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
              onClick={() => setEditOpen(true)}
            >
              <Pencil className="h-3.5 w-3.5" />
              {t('schedules.actions.edit')}
            </ChromeButton>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <ChromeButton
                  aria-label={t('schedules.actions.more')}
                  className="px-2"
                >
                  <Ellipsis className="h-4 w-4" />
                </ChromeButton>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-48">
                <DropdownMenuItem
                  disabled={toggle.isPending || manageAccess.disabled}
                  disabledReason={!toggle.isPending ? manageAccess.reason : undefined}
                  onSelect={() => toggle.mutate(schedule)}
                >
                  {schedule.enabled ? (
                    <PauseCircle className="h-4 w-4" />
                  ) : (
                    <PlayCircle className="h-4 w-4" />
                  )}
                  {schedule.enabled
                    ? t('schedules.actions.pause')
                    : t('schedules.actions.resume')}
                </DropdownMenuItem>
                <DropdownMenuItem
                  disabled={copy.isPending || manageAccess.disabled}
                  disabledReason={!copy.isPending ? manageAccess.reason : undefined}
                  onSelect={() => copy.mutate(schedule)}
                >
                  <Copy className="h-4 w-4" />
                  {t('schedules.actions.copy')}
                </DropdownMenuItem>
                <DropdownMenuItem
                  onSelect={() =>
                    exportScheduleCalendar(
                      schedule,
                      timeline,
                      users.byId,
                    )
                  }
                >
                  <Download className="h-4 w-4" />
                  {t('schedules.actions.export_calendar')}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  className="text-red-soft focus:text-red-soft"
                  disabled={manageAccess.disabled}
                  disabledReason={manageAccess.reason}
                  onSelect={() => setRemovingSchedule(true)}
                >
                  <Trash2 className="h-4 w-4" />
                  {t('schedules.actions.delete')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
      />

      <PageBody className="overflow-auto">
        <div className="flex w-full min-w-[1020px] flex-col gap-4">
          <div className="grid grid-cols-4 gap-4">
            <ScheduleSummaryCard
              icon={UserRoundPlus}
              label={t('schedules.detail.current')}
              value={
                <span className="flex min-w-0 items-center gap-2 text-base">
                  <UserAvatar
                    user={currentUser}
                    size="sm"
                    online={Boolean(currentUser)}
                  />
                  <span className="truncate">
                    {currentUser?.name
                      ?? t('schedules.nobody_on_call')}
                  </span>
                </span>
              }
              hint={
                currentStartedAt
                  ? t('schedules.detail.started_at', {
                      value: formatScheduleDateTime(
                        currentStartedAt,
                        i18n.language,
                        schedule.timezone,
                      ),
                    })
                  : t('schedules.detail.no_active_shift')
              }
              tone="green"
            />
            <ScheduleSummaryCard
              icon={CalendarClock}
              label={t('schedules.detail.next_rotation')}
              value={
                nextSwitch
                  ? relativeDuration(
                      nextSwitch,
                      nowMicros,
                      i18n.language,
                    )
                  : '—'
              }
              hint={
                nextSwitch
                  ? formatScheduleDateTime(
                      nextSwitch,
                      i18n.language,
                      schedule.timezone,
                    )
                  : t('schedules.no_switch')
              }
              tone="blue"
            />
            <ScheduleSummaryCard
              icon={UsersRound}
              label={t('schedules.detail.rotation_people')}
              value={t('schedules.people_count', {
                count: memberIds.length,
              })}
              hint={t('schedules.detail.rotation_rule', {
                value:
                  primaryRotation?.name
                  || t('schedules.no_rotations'),
              })}
              tone="indigo"
            />
            <ScheduleSummaryCard
              icon={CalendarDays}
              label={t('schedules.detail.overrides')}
              value={t('schedules.detail.active_override_count', {
                count: activeOverrides.length,
              })}
              hint={
                activeOverrides[0]
                  ? t('schedules.detail.coverage_window', {
                      start: formatScheduleTime(
                        activeOverrides[0].start_at,
                        i18n.language,
                        schedule.timezone,
                      ),
                      end: formatScheduleTime(
                        activeOverrides[0].end_at,
                        i18n.language,
                        schedule.timezone,
                      ),
                    })
                  : t('schedules.detail.no_override')
              }
              tone="orange"
            />
          </div>

          <div className="grid grid-cols-4 gap-4">
            <RotationRuleCard
              schedule={schedule}
              rotation={primaryRotation}
              users={users.users}
              onView={() => setEditOpen(true)}
            />
            <div className="col-span-3 min-w-0">
              <ScheduleTimeline
                schedule={schedule}
                segments={timeline}
                users={users.users}
                nowMicros={nowMicros}
                locale={i18n.language}
                onExport={() =>
                  exportScheduleCalendar(
                    schedule,
                    timeline,
                    users.byId,
                  )
                }
              />
            </div>
          </div>

          <div className="grid grid-cols-[minmax(360px,.85fr)_minmax(0,1.45fr)] gap-4">
            <ScheduleCard
              title={t('schedules.sections.overrides')}
              action={
                <button
                  type="button"
                  onClick={() =>
                    setShowOverrideHistory((current) => !current)
                  }
                  className="text-xs font-strong text-indigo-soft hover:text-tx-0"
                >
                  {showOverrideHistory
                    ? t('schedules.hide_history')
                    : t('schedules.view_history')}
                </button>
              }
            >
              <OverrideList
                overrides={visibleOverrides}
                users={users.users}
                nowMicros={nowMicros}
                locale={i18n.language}
                timeZone={schedule.timezone}
                busy={
                  removeOverride.isPending || endOverride.isPending
                }
                onEdit={setEditingOverride}
                onEnd={(override) => endOverride.mutate(override)}
                onDelete={setRemovingOverride}
              />
            </ScheduleCard>

            <ScheduleCard title={t('schedules.add_override')}>
              <AddOverrideForm
                schedule={schedule}
                allSchedules={schedulesQuery.data ?? []}
                users={users.users}
                editing={editingOverride}
                focusRequested={addOverrideRequested}
                onFocusHandled={() => {
                  if (addOverrideRequested) {
                    setSearchParams({}, { replace: true });
                  }
                }}
                onCancelEdit={() => setEditingOverride(null)}
                onSaved={() => {
                  setEditingOverride(null);
                  invalidate();
                }}
              />
            </ScheduleCard>
          </div>

          <ScheduleCard
            title={t('schedules.activity.title')}
            action={
              activity.length > 5 ? (
                <button
                  type="button"
                  onClick={() =>
                    setShowAllActivity((current) => !current)
                  }
                  className="text-xs font-strong text-indigo-soft hover:text-tx-0"
                >
                  {showAllActivity
                    ? t('schedules.activity.collapse')
                    : t('schedules.activity.view_all')}
                </button>
              ) : null
            }
            bodyClassName="p-0"
          >
            <ActivityTable
              rows={visibleActivity}
              users={users.users}
              locale={i18n.language}
            />
          </ScheduleCard>
        </div>
      </PageBody>

      <ScheduleEditorDrawer
        open={editOpen}
        editing={schedule}
        users={users.users}
        teams={teamsQuery.data ?? []}
        onClose={() => setEditOpen(false)}
      />
      <ConfirmDialog
        open={removingSchedule}
        onOpenChange={setRemovingSchedule}
        destructive
        title={t('schedules.delete_title')}
        description={schedule.name}
        confirmLabel={tc('actions.delete')}
        busy={removeSchedule.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (manageAccess.allowed) removeSchedule.mutate();
        }}
      />
      <ConfirmDialog
        open={removingOverride !== null}
        onOpenChange={(open) => !open && setRemovingOverride(null)}
        destructive
        title={t('schedules.override_delete_title')}
        description={removingOverride?.reason ?? ''}
        confirmLabel={tc('actions.delete')}
        busy={removeOverride.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() =>
          manageAccess.allowed
          && removingOverride
          && removeOverride.mutate(removingOverride.id)
        }
      />
    </>
  );
}

function RotationRuleCard({
  schedule,
  rotation,
  users,
  onView,
}: {
  schedule: Schedule;
  rotation: Rotation | undefined;
  users: UserLite[];
  onView: () => void;
}) {
  const { t, i18n } = useTranslation('alerts');
  const manageAccess = useActionAccess({ permission: 'schedules.manage' });
  const byId = React.useMemo(
    () => new Map(users.map((user) => [user.id, user])),
    [users],
  );
  return (
    <ScheduleCard
      title={t('schedules.detail.rotation_rule_title')}
      action={
        <ChromeButton
          size="sm"
          variant="ghost"
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onClick={onView}
        >
          {t('schedules.detail.view_rule')}
        </ChromeButton>
      }
    >
      {!rotation ? (
        <div className="py-8 text-center text-xs text-tx-3">
          {t('schedules.no_rotations')}
        </div>
      ) : (
        <dl className="space-y-4">
          <div>
            <dt className="text-xs text-tx-3">
              {t('schedules.detail.rotation_name')}
            </dt>
            <dd className="mt-1 flex items-center gap-2 text-sm font-strong text-tx-0">
              {rotation.name
                || t('schedules.rotation_untitled', { n: 1 })}
              <Pill tone="blue">
                {t(
                  `schedules.rotation_kinds.${rotationKindKey(rotation)}`,
                )}
              </Pill>
            </dd>
          </div>
          <div>
            <dt className="text-xs text-tx-3">
              {t('schedules.detail.rotation_members')}
            </dt>
            <dd className="mt-2 flex flex-wrap items-center gap-2">
              {rotation.members.map((memberId, index) => {
                const user = byId.get(memberId);
                return (
                  <React.Fragment key={memberId}>
                    {index > 0 && (
                      <span className="text-tx-3">→</span>
                    )}
                    <span className="inline-flex items-center gap-1.5 rounded-full border border-bd-0 bg-bg-2 py-1 pl-1 pr-2.5 text-xs font-strong text-tx-1">
                      <UserAvatar user={user} size="sm" />
                      {user?.name ?? t('schedules.unknown_user')}
                    </span>
                  </React.Fragment>
                );
              })}
            </dd>
          </div>
          <div>
            <dt className="text-xs text-tx-3">
              {t('schedules.detail.handoff_time')}
            </dt>
            <dd className="mt-1 text-sm font-strong text-tx-0">
              {t('schedules.detail.handoff_value', {
                cadence: t(
                  `schedules.rotation_kinds.${rotationKindKey(rotation)}`,
                ),
                time: formatScheduleTime(
                  rotation.start_at,
                  i18n.language,
                  schedule.timezone,
                ),
                timezone: schedule.timezone,
              })}
            </dd>
          </div>
        </dl>
      )}
    </ScheduleCard>
  );
}

function ScheduleTimeline({
  schedule,
  segments,
  users,
  nowMicros,
  locale,
  onExport,
}: {
  schedule: Schedule;
  segments: ReturnType<typeof buildScheduleTimeline>;
  users: UserLite[];
  nowMicros: number;
  locale: string;
  onExport: () => void;
}) {
  const { t } = useTranslation('alerts');
  const byId = React.useMemo(
    () => new Map(users.map((user) => [user.id, user])),
    [users],
  );
  const current = resolveScheduleAt(schedule, nowMicros);
  const currentStartedAt = resolutionStartedAt(
    schedule,
    current,
    nowMicros,
  );
  return (
    <ScheduleCard
      title={t('schedules.timeline.title')}
      action={
        <div className="flex items-center gap-4">
          <span className="inline-flex items-center gap-1.5 text-type-micro text-tx-3">
            <Dot tone="green" />
            {t('schedules.timeline.current')}
          </span>
          <span className="inline-flex items-center gap-1.5 text-type-micro text-tx-3">
            <Dot tone="blue" />
            {t('schedules.timeline.upcoming')}
          </span>
          <ChromeButton size="sm" onClick={onExport}>
            {t('schedules.timeline.view_calendar')}
          </ChromeButton>
        </div>
      }
      bodyClassName="overflow-x-auto p-0"
    >
      {segments.length === 0 ? (
        <div className="grid h-48 place-items-center text-xs text-tx-3">
          {t('schedules.timeline.empty')}
        </div>
      ) : (
        <div
          className="grid min-w-[896px] divide-x divide-bd-0"
          style={{
            gridTemplateColumns: `repeat(${segments.length}, minmax(128px, 1fr))`,
          }}
        >
          {segments.map((segment, index) => {
            const startAt =
              index === 0 && currentStartedAt
                ? currentStartedAt
                : segment.startAt;
            const user = segment.userId
              ? byId.get(segment.userId)
              : null;
            const tone =
              segment.source === 'override'
                ? 'orange'
                : index === 0
                  ? 'green'
                  : segment.source === 'gap'
                    ? 'red'
                    : 'blue';
            return (
              <div
                key={segment.id}
                className="flex min-h-[174px] flex-col items-center px-3 py-3 text-center"
              >
                <div className="text-xs font-strong text-tx-2">
                  {formatScheduleDay(
                    startAt,
                    locale,
                    schedule.timezone,
                  )}
                </div>
                <div className="relative mt-3 flex w-full items-center justify-center">
                  <span
                    className={
                      index === 0
                        ? 'absolute left-0 right-1/2 h-px bg-green/40'
                        : 'absolute left-0 right-1/2 h-px bg-indigo/35'
                    }
                  />
                  <span
                    className={
                      tone === 'orange'
                        ? 'absolute left-1/2 right-0 h-px bg-orange/45'
                        : tone === 'green'
                          ? 'absolute left-1/2 right-0 h-px bg-green/40'
                          : 'absolute left-1/2 right-0 h-px bg-indigo/35'
                    }
                  />
                  <span className="relative z-10 rounded-full bg-bg-1 p-0.5">
                    <UserAvatar
                      user={user}
                      size="md"
                      online={index === 0 && Boolean(user)}
                      muted={!user}
                    />
                  </span>
                </div>
                <div className="mt-2 max-w-full truncate text-xs font-strong text-tx-0">
                  {user?.name ?? t('schedules.nobody_on_call')}
                </div>
                <Pill
                  tone={
                    tone as 'orange' | 'green' | 'red' | 'blue'
                  }
                  className="mt-2"
                >
                  {formatScheduleTime(
                    startAt,
                    locale,
                    schedule.timezone,
                  )}
                </Pill>
                <div className="mt-1 font-mono text-type-micro text-tx-3">
                  –{' '}
                  {formatScheduleTime(
                    segment.endAt,
                    locale,
                    schedule.timezone,
                  )}
                </div>
                {segment.source === 'override' && (
                  <div className="mt-1 text-type-micro font-strong text-orange-soft">
                    {t('schedules.override_badge')}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </ScheduleCard>
  );
}

function OverrideList({
  overrides,
  users,
  nowMicros,
  locale,
  timeZone,
  busy,
  onEdit,
  onEnd,
  onDelete,
}: {
  overrides: ScheduleOverride[];
  users: UserLite[];
  nowMicros: number;
  locale: string;
  timeZone: string;
  busy: boolean;
  onEdit: (override: ScheduleOverride) => void;
  onEnd: (override: ScheduleOverride) => void;
  onDelete: (override: ScheduleOverride) => void;
}) {
  const { t } = useTranslation('alerts');
  const manageAccess = useActionAccess({ permission: 'schedules.manage' });
  const byId = React.useMemo(
    () => new Map(users.map((user) => [user.id, user])),
    [users],
  );
  if (overrides.length === 0) {
    return (
      <div className="grid min-h-40 place-items-center rounded-md border border-dashed border-bd-1 text-xs text-tx-3">
        {t('schedules.no_overrides')}
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-3">
      {overrides.map((override) => {
        const active =
          override.start_at <= nowMicros
          && nowMicros < override.end_at;
        const future = override.start_at > nowMicros;
        const user = byId.get(override.user_id);
        return (
          <div
            key={override.id}
            className={
              active
                ? 'relative rounded-md border border-green/30 bg-green-dim/40 p-3 pl-5'
                : future
                  ? 'relative rounded-md border border-orange/25 bg-orange-dim/30 p-3 pl-5'
                  : 'relative rounded-md border border-bd-0 bg-bg-2 p-3 pl-5 opacity-70'
            }
          >
            <span
              className={
                active
                  ? 'absolute inset-y-3 left-2 w-0.5 rounded bg-green'
                  : future
                    ? 'absolute inset-y-3 left-2 w-0.5 rounded bg-orange'
                    : 'absolute inset-y-3 left-2 w-0.5 rounded bg-tx-3'
              }
            />
            <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-3">
              <div>
                <div className="text-type-micro text-tx-3">
                  {t('schedules.override_fields.start')}
                </div>
                <div className="mt-0.5 font-mono text-xs font-strong text-tx-1">
                  {formatScheduleDateTime(
                    override.start_at,
                    locale,
                    timeZone,
                  )}
                </div>
              </div>
              <div className="flex items-center gap-2">
                <UserAvatar user={user} size="sm" />
                <div>
                  <div className="text-xs font-strong text-tx-0">
                    {user?.name ?? t('schedules.unknown_user')}
                  </div>
                  <div className="text-type-micro text-tx-3">
                    {t('schedules.override_person')}
                  </div>
                </div>
              </div>
              <div>
                <div className="text-type-micro text-tx-3">
                  {t('schedules.override_fields.end')}
                </div>
                <div className="mt-0.5 font-mono text-xs font-strong text-tx-1">
                  {formatScheduleDateTime(
                    override.end_at,
                    locale,
                    timeZone,
                  )}
                </div>
              </div>
            </div>
            <div className="mt-3 rounded border border-bd-0 bg-bg-1/70 px-3 py-2">
              <div className="text-type-micro text-tx-3">
                {t('schedules.override_fields.reason')}
              </div>
              <div className="mt-0.5 text-xs text-tx-1">
                {override.reason || '—'}
              </div>
            </div>
            <div className="mt-2 flex justify-end gap-1">
              {active && (
                <ChromeButton
                  size="sm"
                  variant="ghost"
                  disabled={busy || manageAccess.disabled}
                  disabledReason={!busy ? manageAccess.reason : undefined}
                  onClick={() => onEnd(override)}
                >
                  {t('schedules.end_override')}
                </ChromeButton>
              )}
              {(active || future) && (
                <ChromeButton
                  size="sm"
                  variant="ghost"
                  disabled={busy || manageAccess.disabled}
                  disabledReason={!busy ? manageAccess.reason : undefined}
                  onClick={() => onEdit(override)}
                >
                  {t('schedules.actions.edit')}
                </ChromeButton>
              )}
              <ChromeButton
                size="sm"
                variant="ghost"
                disabled={busy || manageAccess.disabled}
                disabledReason={!busy ? manageAccess.reason : undefined}
                onClick={() => onDelete(override)}
              >
                {t('schedules.actions.delete')}
              </ChromeButton>
            </div>
          </div>
        );
      })}
    </div>
  );
}

function AddOverrideForm({
  schedule,
  allSchedules,
  users,
  editing,
  focusRequested,
  onFocusHandled,
  onCancelEdit,
  onSaved,
}: {
  schedule: Schedule;
  allSchedules: Schedule[];
  users: UserLite[];
  editing: ScheduleOverride | null;
  focusRequested: boolean;
  onFocusHandled: () => void;
  onCancelEdit: () => void;
  onSaved: () => void;
}) {
  const { t, i18n } = useTranslation('alerts');
  const manageAccess = useActionAccess({ permission: 'schedules.manage' });
  const formRef = React.useRef<HTMLFormElement>(null);
  const [userId, setUserId] = React.useState('');
  const [reason, setReason] = React.useState('');
  const [start, setStart] = React.useState('');
  const [end, setEnd] = React.useState('');

  const reset = React.useCallback(() => {
    setUserId('');
    setReason('');
    setStart('');
    setEnd('');
  }, []);

  React.useEffect(() => {
    if (editing) {
      setUserId(editing.user_id);
      setReason(editing.reason);
      setStart(microsToLocalInput(editing.start_at));
      setEnd(microsToLocalInput(editing.end_at));
    } else {
      reset();
    }
  }, [editing, reset, schedule.id]);

  React.useEffect(() => {
    if (!focusRequested) return;
    formRef.current?.scrollIntoView({
      behavior: 'smooth',
      block: 'center',
    });
    formRef.current
      ?.querySelector<HTMLInputElement>('input')
      ?.focus();
    onFocusHandled();
  }, [focusRequested, onFocusHandled]);

  const startMicros = localInputToMicros(start);
  const endMicros = localInputToMicros(end);
  const timeValid =
    Boolean(startMicros && endMicros) && endMicros > startMicros;
  const overlaps = schedule.overrides.some(
    (override) =>
      override.id !== editing?.id
      && startMicros < override.end_at
      && override.start_at < endMicros,
  );
  const pastStart =
    Boolean(startMicros) && startMicros < Date.now() * 1000;
  const crossSchedule = Boolean(
    userId
    && startMicros
    && allSchedules.some((candidate) => {
      if (candidate.id === schedule.id || !candidate.enabled) {
        return false;
      }
      return (
        resolveScheduleAt(candidate, startMicros)?.userId === userId
      );
    }),
  );

  const save = useMutation({
    mutationFn: () => {
      const payload: schedulesApi.OverrideInput = {
        user_id: userId,
        reason: reason.trim(),
        start_at_micros: startMicros,
        end_at_micros: endMicros,
      };
      return editing
        ? schedulesApi.updateOverride(schedule.id, editing.id, payload)
        : schedulesApi.addOverride(schedule.id, payload);
    },
    onSuccess: () => {
      toast.success(
        editing
          ? t('schedules.override_updated')
          : t('schedules.override_added'),
      );
      reset();
      onSaved();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!manageAccess.allowed) return;
    if (!userId) {
      toast.error(t('schedules.errors.override_user'));
      return;
    }
    if (!reason.trim()) {
      toast.error(t('schedules.errors.override_reason'));
      return;
    }
    if (!timeValid) {
      toast.error(t('schedules.errors.override_window'));
      return;
    }
    if (overlaps) {
      toast.error(t('schedules.errors.override_conflict'));
      return;
    }
    save.mutate();
  };

  const selectedUser = users.find((user) => user.id === userId);
  return (
    <form ref={formRef} onSubmit={submit}>
      <fieldset
        disabled={manageAccess.disabled}
        aria-disabled={manageAccess.disabled || undefined}
        className="contents"
      >
      <FormRow className="grid-cols-[240px_minmax(0,1fr)]">
        <FormField
          label={t('schedules.override_fields.user')}
          required
        >
          <FormSelect
            value={userId}
            onChange={setUserId}
            options={[
              {
                value: '',
                label: t('schedules.pick_user'),
              },
              ...users.map((user) => ({
                value: user.id,
                label: user.name,
              })),
            ]}
          />
        </FormField>
        <FormField
          label={t('schedules.override_fields.reason')}
          required
        >
          <FormInput
            value={reason}
            onChange={(event) => setReason(event.target.value)}
            placeholder={t('schedules.placeholders.override_reason')}
          />
        </FormField>
      </FormRow>
      <FormRow className="mt-4">
        <FormField
          label={t('schedules.override_fields.start')}
          required
        >
          <DateTimePicker
            value={start}
            onChange={setStart}
            required
          />
        </FormField>
        <FormField
          label={t('schedules.override_fields.end')}
          required
        >
          <DateTimePicker
            value={end}
            onChange={setEnd}
            required
          />
        </FormField>
      </FormRow>

      <div className="mt-3 space-y-2">
        {overlaps && (
          <ValidationNotice tone="red">
            {t('schedules.errors.override_conflict')}
          </ValidationNotice>
        )}
        {crossSchedule && (
          <ValidationNotice tone="orange">
            {t('schedules.warnings.other_schedule')}
          </ValidationNotice>
        )}
        {pastStart && (
          <ValidationNotice tone="orange">
            {t('schedules.warnings.immediate')}
          </ValidationNotice>
        )}
        {userId && timeValid && (
          <div className="rounded-md border border-bd-0 bg-bg-2 px-3 py-2 text-xs leading-relaxed text-tx-2">
            {t('schedules.override_summary', {
              user: selectedUser?.name ?? userId,
              start: formatScheduleDateTime(
                startMicros,
                i18n.language,
                schedule.timezone,
              ),
              end: formatScheduleDateTime(
                endMicros,
                i18n.language,
                schedule.timezone,
              ),
            })}
          </div>
        )}
      </div>

      <div className="mt-4 flex justify-end gap-2">
        {editing && (
          <ChromeButton type="button" onClick={onCancelEdit}>
            {t('schedules.cancel_edit')}
          </ChromeButton>
        )}
        <ChromeButton
          type="submit"
          variant="primary"
          disabled={save.isPending || overlaps || manageAccess.disabled}
          disabledReason={!save.isPending ? manageAccess.reason : undefined}
        >
          {editing
            ? t('schedules.update_override')
            : t('schedules.add_override')}
        </ChromeButton>
      </div>
      </fieldset>
    </form>
  );
}

function ValidationNotice({
  tone,
  children,
}: {
  tone: 'red' | 'orange';
  children: React.ReactNode;
}) {
  return (
    <div
      className={
        tone === 'red'
          ? 'rounded-md border border-red/30 bg-red-dim px-3 py-2 text-xs text-red-soft'
          : 'rounded-md border border-orange/30 bg-orange-dim px-3 py-2 text-xs text-orange-soft'
      }
    >
      {children}
    </div>
  );
}

function ActivityTable({
  rows,
  users,
  locale,
}: {
  rows: ActivityRow[];
  users: UserLite[];
  locale: string;
}) {
  const { t } = useTranslation('alerts');
  const byId = React.useMemo(
    () => new Map(users.map((user) => [user.id, user])),
    [users],
  );
  if (rows.length === 0) {
    return (
      <div className="grid h-28 place-items-center text-xs text-tx-3">
        {t('schedules.activity.empty')}
      </div>
    );
  }
  return (
    <div className="overflow-x-auto">
      <table className="w-full min-w-[860px] text-left">
        <thead className="bg-bg-2 text-xs font-strong text-tx-3">
          <tr>
            <th className="px-4 py-2">
              {t('schedules.activity.columns.time')}
            </th>
            <th className="px-4 py-2">
              {t('schedules.activity.columns.actor')}
            </th>
            <th className="px-4 py-2">
              {t('schedules.activity.columns.type')}
            </th>
            <th className="px-4 py-2">
              {t('schedules.activity.columns.content')}
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-bd-0">
          {rows.map((row) => {
            const actor = row.actorId
              ? byId.get(row.actorId)
              : null;
            return (
              <tr key={row.id} className="text-xs text-tx-1">
                <td className="whitespace-nowrap px-4 py-2.5 font-mono text-tx-2">
                  {formatScheduleDateTime(row.at, locale)}
                </td>
                <td className="px-4 py-2.5">
                  <span className="inline-flex items-center gap-2">
                    <UserAvatar
                      user={actor}
                      size="sm"
                      muted={!actor}
                    />
                    {actor?.name ?? t('schedules.system_actor')}
                  </span>
                </td>
                <td className="px-4 py-2.5">
                  <Pill tone={activityTone(row.action)}>
                    {t(`schedules.activity.types.${activityKey(row.action)}`)}
                  </Pill>
                </td>
                <td className="max-w-[620px] truncate px-4 py-2.5 text-tx-2">
                  {activityDescription(row, t, byId)}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function ScheduleStatusPill({
  status,
}: {
  status: ReturnType<typeof scheduleStatus>;
}) {
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

function buildActivityRows(
  schedule: Schedule,
  events?: AuditEvent[],
): ActivityRow[] {
  if (events && events.length > 0) {
    return events.map((event) => ({
      id: event.id,
      at: event.ts_micros,
      actorId:
        event.actor_kind === 'user' ? event.actor_id : null,
      action: event.action,
      payload: event.payload,
    }));
  }
  const synthetic: ActivityRow[] = [
    {
      id: `${schedule.id}-created`,
      at: schedule.created_at,
      actorId: schedule.created_by ?? null,
      action: 'schedule.created',
      payload: { name: schedule.name },
    },
  ];
  for (const override of schedule.overrides) {
    synthetic.push({
      id: `${schedule.id}-${override.id}`,
      at: override.start_at,
      actorId: schedule.updated_by ?? null,
      action: 'schedule.override_added',
      payload: {
        user_id: override.user_id,
        start_at: override.start_at,
        end_at: override.end_at,
        reason: override.reason,
      },
    });
  }
  return synthetic.sort((a, b) => b.at - a.at);
}

function activityKey(action: string): string {
  if (action.endsWith('override_added')) return 'override';
  if (action.endsWith('override_updated')) return 'override';
  if (action.endsWith('override_removed')) return 'override';
  if (action.endsWith('paused')) return 'pause';
  if (action.endsWith('resumed')) return 'resume';
  if (action.endsWith('created')) return 'create';
  if (action.endsWith('deleted')) return 'delete';
  return 'edit';
}

function activityTone(
  action: string,
): 'green' | 'blue' | 'orange' | 'red' | 'dim' {
  const key = activityKey(action);
  if (key === 'create' || key === 'resume') return 'green';
  if (key === 'override') return 'orange';
  if (key === 'delete') return 'red';
  if (key === 'pause') return 'dim';
  return 'blue';
}

function activityDescription(
  row: ActivityRow,
  t: ReturnType<typeof useTranslation<'alerts'>>['t'],
  usersById: Map<string, UserLite>,
): string {
  const key = activityKey(row.action);
  if (key === 'override') {
    const userId = String(row.payload.user_id ?? '');
    return t('schedules.activity.descriptions.override', {
      user:
        usersById.get(userId)?.name
        ?? t('schedules.unknown_user'),
      reason: String(row.payload.reason ?? '—'),
    });
  }
  return t(`schedules.activity.descriptions.${key}`, {
    name: String(row.payload.name ?? ''),
  });
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

function exportScheduleCalendar(
  schedule: Schedule,
  timeline: ReturnType<typeof buildScheduleTimeline>,
  usersById: Map<string, UserLite>,
) {
  const escape = (value: string) =>
    value
      .replaceAll('\\', '\\\\')
      .replaceAll(',', '\\,')
      .replaceAll(';', '\\;')
      .replaceAll('\n', '\\n');
  const stamp = (micros: number) =>
    new Date(micros / 1000)
      .toISOString()
      .replaceAll('-', '')
      .replaceAll(':', '')
      .replace('.000', '');
  const events = timeline.map((segment) => {
    const user = segment.userId
      ? usersById.get(segment.userId)?.name ?? segment.userId
      : 'Unassigned';
    return [
      'BEGIN:VEVENT',
      `UID:${segment.id}@molesignal`,
      `DTSTAMP:${stamp(Date.now() * 1000)}`,
      `DTSTART:${stamp(segment.startAt)}`,
      `DTEND:${stamp(segment.endAt)}`,
      `SUMMARY:${escape(`${schedule.name} · ${user}`)}`,
      `DESCRIPTION:${escape(schedule.description)}`,
      'END:VEVENT',
    ].join('\r\n');
  });
  const ics = [
    'BEGIN:VCALENDAR',
    'VERSION:2.0',
    'PRODID:-//MoleSignal//On-call Schedule//EN',
    'CALSCALE:GREGORIAN',
    ...events,
    'END:VCALENDAR',
  ].join('\r\n');
  const url = URL.createObjectURL(
    new Blob([ics], { type: 'text/calendar;charset=utf-8' }),
  );
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = `${schedule.name.replaceAll(/\s+/g, '-')}.ics`;
  anchor.click();
  URL.revokeObjectURL(url);
}
