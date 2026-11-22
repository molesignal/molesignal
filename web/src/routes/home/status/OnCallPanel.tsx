import { useQuery } from '@tanstack/react-query';
import { TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as schedulesApi from '@/api/schedules';
import { useTimeFormatter } from '@/lib/time';
import { canAccessProductPath, hasPermission, useProductAccess } from '@/product/access';
import { replacedUserAt, resolveScheduleAt } from '@/routes/alerts/schedule/model';
import { StatusWord } from '@/shell/StatusWord';
import { useReachability } from '@/shell/useReachability';
import { useUsers } from '@/shell/useUsers';
import { useAuthStore } from '@/stores/auth';
import { formatRelativeSpan } from '@/time/relative';
import type { Schedule } from '@/types/alerting';

import { myShift } from './myShift';
import { Panel } from './Panel';

const SCHEDULES_PATH = '/alerts/schedules';

/** A shift's span in the reader's time zone, the day said once when it starts and ends on it. */
function shiftSpan(startAt: number, endAt: number, locale: string, timeZone: string, hour12: boolean): string {
  const day = new Intl.DateTimeFormat(locale, { timeZone, month: 'short', day: 'numeric', weekday: 'short' });
  const time = new Intl.DateTimeFormat(locale, { timeZone, hour: '2-digit', minute: '2-digit', hour12 });
  const start = new Date(startAt / 1000);
  const end = new Date(endAt / 1000);
  const endText = day.format(start) === day.format(end) ? time.format(end) : `${day.format(end)} ${time.format(end)}`;

  return `${day.format(start)} ${time.format(start)} – ${endText}`;
}

function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const letters = words.length > 1 ? `${words[0]![0]}${words[words.length - 1]![0]}` : (words[0]?.slice(0, 2) ?? '');
  return letters.toUpperCase();
}

/**
 * The reader's on-call: the shift under way or the next one, and who holds the schedule now when
 * it is someone else. Shown to whoever can read schedules.
 */
export function OnCallPanel({ className }: { className?: string }) {
  const { t, i18n } = useTranslation('onboarding');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const access = useProductAccess();
  const userId = useAuthStore((state) => state.ctx?.userId ?? '');
  const formatter = useTimeFormatter();
  const { nameOf } = useUsers();
  const canRead = canAccessProductPath(SCHEDULES_PATH, access);
  // Whether the reader can get alerts is asked only when the card shows and the answer may be read.
  const canReadReach = canRead && hasPermission('notify.policies.read', access);
  const reach = useReachability(canReadReach && userId ? [userId] : []);
  const schedules = useQuery({
    queryKey: ['schedules'],
    queryFn: schedulesApi.list,
    refetchInterval: 60_000,
    enabled: canRead,
  });

  if (!canRead) return null;

  const nowMicros = Date.now() * 1000;
  const shift = myShift(schedules.data ?? [], userId, nowMicros);
  const schedule: Schedule | undefined = shift?.schedule ?? schedules.data?.find((candidate) => candidate.enabled);
  const holder = schedule ? resolveScheduleAt(schedule, nowMicros) : null;
  const replaced = schedule ? replacedUserAt(schedule, nowMicros) : null;
  const hour12 = formatter.format === 'local_12h';

  return (
    <Panel
      title={t('home.oncall.title')}
      className={className}
      action={{
        label: t('home.oncall.schedules'),
        to: shift ? `${SCHEDULES_PATH}/${encodeURIComponent(shift.schedule.id)}` : SCHEDULES_PATH,
      }}
      bodyClassName="px-[12px] py-[10px]"
      testId="home-oncall"
    >
      {shift ? (
        <div data-testid="home-my-shift">
          <div className="flex items-baseline justify-between gap-3 font-sans text-xs text-tx-3">
            <span className="min-w-0 truncate">
              {shift.now ? (
                <StatusWord tone="green" strong>
                  {t('home.oncall.on_now')}
                </StatusWord>
              ) : (
                t('home.oncall.next')
              )}
              {' · '}
              {shift.schedule.name}
            </span>
            <span className="shrink-0">
              {shift.now
                ? t('home.oncall.hands_over', { time: formatRelativeSpan(shift.endAt, nowMicros, locale) })
                : formatRelativeSpan(shift.startAt, nowMicros, locale)}
            </span>
          </div>
          <div className="mt-1 font-sans text-md font-strong tabular-nums text-tx-0">
            {shiftSpan(shift.startAt, shift.endAt, locale, formatter.tz, hour12)}
          </div>
          {reach[userId] && reach[userId] !== 'personal' && (
            // Holding a shift with no way of one's own to get alerts: said where the shift is.
            <p role="status" data-oncall-unreachable className="mt-2 flex items-start gap-1.5 font-sans text-xs text-tx-1">
              <TriangleAlert aria-hidden="true" className="mt-px h-3.5 w-3.5 shrink-0 text-[var(--orange-ink)]" />
              <span>
                {t('home.oncall.unreachable')}{' '}
                <Link to="/account/settings/notify" className="font-strong text-[var(--blue-ink)] hover:underline">
                  {t('home.oncall.set_up_notify')}
                </Link>
              </span>
            </p>
          )}
        </div>
      ) : (
        <p className="font-sans text-sm text-tx-3">
          {schedules.isLoading
            ? t('home.loading')
            : schedules.isError
              ? t('home.oncall.unavailable')
              : t('home.oncall.none')}
        </p>
      )}
      {schedule && !holder && !schedules.isLoading && (
        // Nobody holds the schedule now: said here, with the way to put someone on it.
        <p role="status" data-oncall-gap className="mt-3 flex items-start gap-1.5 font-sans text-xs text-tx-1">
          <TriangleAlert aria-hidden="true" className="mt-px h-3.5 w-3.5 shrink-0 text-[var(--orange-ink)]" />
          <span>
            {t('home.oncall.gap', { schedule: schedule.name })}{' '}
            <Link
              to={`${SCHEDULES_PATH}/${encodeURIComponent(schedule.id)}?addOverride=1`}
              className="font-strong text-[var(--blue-ink)] hover:underline"
            >
              {t('home.oncall.arrange')}
            </Link>
          </span>
        </p>
      )}
      {schedule && holder && holder.userId !== userId && (
        <>
          <div className="my-3 h-px bg-[var(--row-rule)]" />
          <div className="flex items-center gap-3" data-testid="home-on-call-now">
            <span className="type-micro grid h-8 w-8 shrink-0 place-items-center rounded-full bg-bg-2 font-sans font-strong text-tx-2">
              {initials(nameOf(holder.userId))}
            </span>
            <div className="min-w-0 flex-1">
              <div className="truncate font-sans text-sm font-strong text-tx-0">{nameOf(holder.userId)}</div>
              <div className="mt-0.5 truncate font-sans text-xs text-tx-3">
                {replaced
                  ? t('home.oncall.covering', { name: nameOf(replaced), schedule: schedule.name })
                  : t('home.oncall.holding', { schedule: schedule.name })}
              </div>
            </div>
          </div>
        </>
      )}
    </Panel>
  );
}
