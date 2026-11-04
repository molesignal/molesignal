import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowLeftRight } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as schedulesApi from '@/api/schedules';
import type { SwapAnswer, SwapRequest, SwapStatus } from '@/api/schedules';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { DateTimePicker } from '@/shell/DateTimePicker';
import { FormField, FormInput, FormRow, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';
import type { UserLite } from '@/shell/useUsers';
import { useAuthStore } from '@/stores/auth';
import type { Schedule } from '@/types/alerting';

import { localInputToMicros, microsToLocalInput } from './EditorDrawer';
import { buildScheduleTimeline, scheduleMemberIds } from './model';
import { ScheduleCard, formatScheduleDateTime } from './Ui';

type Translate = ReturnType<typeof useTranslation<'alerts'>>['t'];

const SWAPS_KEY = 'schedule-swaps';
const MY_SWAPS_KEY = ['my-swaps'];

/** How far ahead the shifts one can give away are looked for. */
const SHIFT_LOOKAHEAD_DAYS = 30;
const MINUTE_MICROS = 60_000_000;
const HOUR_MICROS = 60 * MINUTE_MICROS;
const DAY_MICROS = 24 * HOUR_MICROS;

const STATUS_INK: Record<SwapStatus, string> = {
  pending: 'text-orange',
  awaiting_approval: 'text-blue',
  accepted: 'text-green',
  declined: 'text-red',
  rejected: 'text-red',
  cancelled: 'text-tx-3',
  expired: 'text-tx-3',
};

interface Shift {
  start: number;
  end: number;
}

const isOpen = (status: SwapStatus) => status === 'pending' || status === 'awaiting_approval';

/** The reader's own coming shifts in a schedule, the one under way counted from now. */
function upcomingShifts(schedule: Schedule, userId: string, nowMicros: number): Shift[] {
  if (!userId) return [];
  const from = Math.ceil(nowMicros / MINUTE_MICROS) * MINUTE_MICROS;
  const shifts: Shift[] = [];
  for (const segment of buildScheduleTimeline(schedule, from, SHIFT_LOOKAHEAD_DAYS)) {
    if (segment.userId !== userId) continue;
    const last = shifts[shifts.length - 1];
    if (last && last.end === segment.startAt) last.end = segment.endAt;
    else shifts.push({ start: segment.startAt, end: segment.endAt });
  }
  return shifts;
}

function useSwapAnswer() {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ swap, answer }: { swap: SwapRequest; answer: SwapAnswer }) =>
      schedulesApi.answerSwap(swap.scheduleId, swap.id, answer),
    onSuccess: (saved, { answer }) => {
      toast.success(
        answer === 'accept' && saved.status === 'awaiting_approval'
          ? t('schedules.swaps.done.accept_waiting')
          : t(`schedules.swaps.done.${answer}`),
      );
      void queryClient.invalidateQueries({ queryKey: [SWAPS_KEY] });
      void queryClient.invalidateQueries({ queryKey: MY_SWAPS_KEY });
      void queryClient.invalidateQueries({ queryKey: ['schedule-activity', saved.scheduleId] });
      if (saved.status === 'accepted') void queryClient.invalidateQueries({ queryKey: ['schedules'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
}

/** A schedule's swap requests, open ones first; asking for one starts here. */
export function ScheduleSwapsCard({
  schedule,
  users,
  nowMicros,
}: {
  schedule: Schedule;
  users: UserLite[];
  nowMicros: number;
}) {
  const { t } = useTranslation('alerts');
  const currentUserId = useAuthStore((state) => state.ctx?.userId ?? '');
  const manageAccess = useActionAccess({ permission: 'schedules.swaps.approve' });
  const [showSettled, setShowSettled] = React.useState(false);
  const [requesting, setRequesting] = React.useState(false);
  const swaps = useQuery({
    queryKey: [SWAPS_KEY, schedule.id],
    queryFn: () => schedulesApi.swaps(schedule.id),
    retry: false,
  });
  const answer = useSwapAnswer();
  const usersById = React.useMemo(() => new Map(users.map((user) => [user.id, user])), [users]);
  // The shifts one could give away reach past the coverage a schedule comes with: ask for them.
  const shiftFrom = Math.ceil(nowMicros / MINUTE_MICROS) * MINUTE_MICROS;
  const lookahead = useQuery({
    queryKey: ['schedule-coverage', schedule.id, schedule.updatedAt, Math.floor(shiftFrom / HOUR_MICROS)],
    queryFn: () =>
      schedulesApi.coverage(schedule.id, shiftFrom, shiftFrom + SHIFT_LOOKAHEAD_DAYS * DAY_MICROS),
    enabled: Boolean(currentUserId),
    retry: false,
  });
  const shifts = React.useMemo(
    () =>
      upcomingShifts(
        { ...schedule, coverage: lookahead.data ?? schedule.coverage },
        currentUserId,
        nowMicros,
      ),
    [schedule, lookahead.data, currentUserId, nowMicros],
  );
  const all = swaps.data ?? [];
  const list = showSettled ? all : all.filter((swap) => isOpen(swap.status));
  const hasSettled = all.some((swap) => !isOpen(swap.status));

  return (
    <ScheduleCard
      title={t('schedules.swaps.title')}
      action={
        <div className="flex items-center gap-3">
          {hasSettled && (
            <button
              type="button"
              onClick={() => setShowSettled((current) => !current)}
              className="text-xs font-strong text-indigo-soft hover:text-tx-0"
            >
              {showSettled ? t('schedules.swaps.hide_settled') : t('schedules.swaps.show_settled')}
            </button>
          )}
          <ChromeButton
            size="sm"
            disabled={shifts.length === 0}
            disabledReason={shifts.length === 0 ? t('schedules.swaps.not_on_call') : undefined}
            onClick={() => setRequesting(true)}
          >
            <ArrowLeftRight className="h-3.5 w-3.5" />
            {t('schedules.swaps.request')}
          </ChromeButton>
        </div>
      }
      bodyClassName="pt-1"
    >
      {list.length === 0 ? (
        <p className="py-3 font-sans text-xs text-tx-3">{t('schedules.swaps.empty')}</p>
      ) : (
        <ul data-schedule-swaps className="flex flex-col divide-y divide-bd-0 font-sans">
          {list.map((swap) => (
            <SwapRow
              key={swap.id}
              swap={swap}
              usersById={usersById}
              timeZone={schedule.timezone}
              currentUserId={currentUserId}
              canDecide={manageAccess.allowed}
              busy={answer.isPending}
              onAnswer={(next) => answer.mutate({ swap, answer: next })}
              t={t}
            />
          ))}
        </ul>
      )}
      {requesting && (
        <SwapRequestDialog
          schedule={schedule}
          users={users}
          shifts={shifts}
          currentUserId={currentUserId}
          onClose={() => setRequesting(false)}
        />
      )}
    </ScheduleCard>
  );
}

/** The swaps the reader asked for and those asking them, across schedules. */
export function MySwapsDialog({
  open,
  schedules,
  users,
  onClose,
}: {
  open: boolean;
  schedules: Schedule[];
  users: UserLite[];
  onClose: () => void;
}) {
  const { t } = useTranslation('alerts');
  const currentUserId = useAuthStore((state) => state.ctx?.userId ?? '');
  const swaps = useMySwaps(open);
  const answer = useSwapAnswer();
  const usersById = React.useMemo(() => new Map(users.map((user) => [user.id, user])), [users]);
  const timeZones = React.useMemo(
    () => new Map(schedules.map((schedule) => [schedule.id, schedule.timezone])),
    [schedules],
  );
  const list = [...(swaps.data ?? [])].sort(
    (a, b) => Number(isOpen(b.status)) - Number(isOpen(a.status)) || b.createdAt - a.createdAt,
  );

  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('schedules.swaps.mine_title')}</DialogTitle>
          <DialogDescription>{t('schedules.swaps.mine_description')}</DialogDescription>
        </DialogHeader>
        <div className="max-h-[60vh] min-w-0 overflow-y-auto">
          {list.length === 0 ? (
            <p className="py-3 font-sans text-xs text-tx-3">{t('schedules.swaps.mine_empty')}</p>
          ) : (
            <ul data-my-swaps className="flex flex-col divide-y divide-bd-0 font-sans">
              {list.map((swap) => (
                <SwapRow
                  key={swap.id}
                  swap={swap}
                  usersById={usersById}
                  timeZone={timeZones.get(swap.scheduleId)}
                  currentUserId={currentUserId}
                  canDecide={false}
                  showSchedule
                  busy={answer.isPending}
                  onAnswer={(next) => answer.mutate({ swap, answer: next })}
                  t={t}
                />
              ))}
            </ul>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

/** The reader's swaps; `waiting` counts those asking them for an answer. */
export function useMySwaps(enabled = true) {
  const currentUserId = useAuthStore((state) => state.ctx?.userId ?? '');
  const query = useQuery({
    queryKey: MY_SWAPS_KEY,
    queryFn: schedulesApi.mySwaps,
    enabled,
    retry: false,
    staleTime: 30_000,
  });
  const waiting = (query.data ?? []).filter(
    (swap) => swap.status === 'pending' && swap.targetId === currentUserId,
  ).length;
  return { ...query, waiting };
}

function SwapRow({
  swap,
  usersById,
  timeZone,
  currentUserId,
  canDecide,
  showSchedule = false,
  busy,
  onAnswer,
  t,
}: {
  swap: SwapRequest;
  usersById: Map<string, UserLite>;
  timeZone: string | undefined;
  currentUserId: string;
  canDecide: boolean;
  showSchedule?: boolean;
  busy: boolean;
  onAnswer: (answer: SwapAnswer) => void;
  t: Translate;
}) {
  const { i18n } = useTranslation('alerts');
  const name = (id: string | null) =>
    (id && usersById.get(id)?.name) || t('schedules.unknown_user');
  const answers: SwapAnswer[] = [];
  if (swap.status === 'pending' && swap.targetId === currentUserId) answers.push('accept', 'decline');
  if (swap.status === 'awaiting_approval' && canDecide) answers.push('approve', 'reject');
  if (isOpen(swap.status) && swap.requesterId === currentUserId) answers.push('cancel');
  const span = `${formatScheduleDateTime(swap.startAt, i18n.language, timeZone)} – ${formatScheduleDateTime(
    swap.endAt,
    i18n.language,
    timeZone,
  )}`;
  const decided =
    swap.decidedBy && !isOpen(swap.status) && swap.status !== 'expired' && swap.decidedBy !== swap.targetId
      ? t('schedules.swaps.decided_by', { name: name(swap.decidedBy) })
      : null;

  return (
    <li data-swap={swap.id} data-swap-status={swap.status} className="flex min-w-0 items-center gap-3 py-2.5">
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-baseline gap-2">
          <span className="min-w-0 truncate text-sm text-tx-0">
            {t('schedules.swaps.asks', { requester: name(swap.requesterId), target: name(swap.targetId) })}
          </span>
          <span className={cn('shrink-0 text-xs font-strong', STATUS_INK[swap.status])}>
            {t(`schedules.swaps.status.${swap.status}`)}
          </span>
        </div>
        <p className="mt-0.5 truncate text-xs text-tx-2">
          {showSchedule && (
            <>
              <Link
                to={`/alerts/schedules/${encodeURIComponent(swap.scheduleId)}`}
                className="text-tx-1 underline-offset-2 hover:underline"
              >
                {swap.scheduleName ?? t('schedules.swaps.open_schedule')}
              </Link>
              {' · '}
            </>
          )}
          {[span, swap.note || null, decided].filter(Boolean).join(' · ')}
        </p>
      </div>
      {answers.length > 0 && (
        <div className="flex shrink-0 items-center gap-1">
          {answers.map((answer) => (
            <ChromeButton
              key={answer}
              size="sm"
              variant={answer === 'accept' || answer === 'approve' ? 'primary' : 'ghost'}
              disabled={busy}
              onClick={() => onAnswer(answer)}
            >
              {t(`schedules.swaps.answer.${answer}`)}
            </ChromeButton>
          ))}
        </div>
      )}
    </li>
  );
}

/** Asking a member to take a stretch of the reader's shift: one of their coming shifts, or times of their own. */
function SwapRequestDialog({
  schedule,
  users,
  shifts,
  currentUserId,
  onClose,
}: {
  schedule: Schedule;
  users: UserLite[];
  shifts: Shift[];
  currentUserId: string;
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const first = shifts[0];
  const [shift, setShift] = React.useState(first ? String(first.start) : '');
  const [start, setStart] = React.useState(first ? microsToLocalInput(first.start) : '');
  const [end, setEnd] = React.useState(first ? microsToLocalInput(first.end) : '');
  const [targetId, setTargetId] = React.useState('');
  const [note, setNote] = React.useState('');
  const usersById = React.useMemo(() => new Map(users.map((user) => [user.id, user])), [users]);
  const members = scheduleMemberIds(schedule).filter((id) => id !== currentUserId);
  const startMicros = localInputToMicros(start);
  const endMicros = localInputToMicros(end);
  const shiftLabel = (item: Shift) =>
    `${formatScheduleDateTime(item.start, i18n.language, schedule.timezone)} – ${formatScheduleDateTime(
      item.end,
      i18n.language,
      schedule.timezone,
    )}`;

  const save = useMutation({
    mutationFn: () =>
      schedulesApi.requestSwap(schedule.id, {
        targetId: targetId,
        startAt: startMicros,
        endAt: endMicros,
        note: note.trim(),
      }),
    onSuccess: () => {
      toast.success(t('schedules.swaps.sent'));
      void queryClient.invalidateQueries({ queryKey: [SWAPS_KEY, schedule.id] });
      void queryClient.invalidateQueries({ queryKey: MY_SWAPS_KEY });
      void queryClient.invalidateQueries({ queryKey: ['schedule-activity', schedule.id] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const pickShift = (value: string) => {
    setShift(value);
    const picked = shifts.find((item) => String(item.start) === value);
    if (!picked) return;
    setStart(microsToLocalInput(picked.start));
    setEnd(microsToLocalInput(picked.end));
  };

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!startMicros || !endMicros || endMicros <= startMicros) {
      toast.error(t('schedules.swaps.errors.window'));
      return;
    }
    if (!targetId) {
      toast.error(t('schedules.swaps.errors.target'));
      return;
    }
    save.mutate();
  };

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('schedules.swaps.request_title')}</DialogTitle>
          <DialogDescription>
            {schedule.swapNeedsApproval
              ? t('schedules.swaps.request_description_approval')
              : t('schedules.swaps.request_description')}
          </DialogDescription>
        </DialogHeader>
        <form id="swap-request-form" data-swap-request onSubmit={submit} className="flex min-w-0 flex-col gap-4">
          <FormField label={t('schedules.swaps.fields.shift')}>
            <FormSelect
              value={shift}
              onChange={pickShift}
              options={[
                { value: '', label: t('schedules.swaps.pick_shift') },
                ...shifts.map((item) => ({ value: String(item.start), label: shiftLabel(item) })),
              ]}
            />
          </FormField>
          <FormRow>
            <FormField label={t('schedules.swaps.fields.start')} required>
              <DateTimePicker
                value={start}
                onChange={(value) => {
                  setStart(value);
                  setShift('');
                }}
                required
              />
            </FormField>
            <FormField label={t('schedules.swaps.fields.end')} required>
              <DateTimePicker
                value={end}
                onChange={(value) => {
                  setEnd(value);
                  setShift('');
                }}
                required
              />
            </FormField>
          </FormRow>
          <FormField label={t('schedules.swaps.fields.target')} required>
            <FormSelect
              value={targetId}
              onChange={setTargetId}
              options={[
                { value: '', label: t('schedules.swaps.pick_target') },
                ...members.map((id) => ({ value: id, label: usersById.get(id)?.name ?? t('schedules.unknown_user') })),
              ]}
            />
          </FormField>
          <FormField label={t('schedules.swaps.fields.note')}>
            <FormInput
              value={note}
              maxLength={500}
              onChange={(event) => setNote(event.target.value)}
              placeholder={t('schedules.swaps.placeholders.note')}
            />
          </FormField>
        </form>
        <DialogFooter>
          <ChromeButton type="button" onClick={onClose}>
            {tc('actions.cancel')}
          </ChromeButton>
          <ChromeButton type="submit" form="swap-request-form" variant="primary" disabled={save.isPending}>
            {t('schedules.swaps.submit')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
