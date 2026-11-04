import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { CalendarDays, Clock3, UsersRound } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as schedulesApi from '@/api/schedules';
import { Pill } from '@/shell/chrome';
import type { UserLite } from '@/shell/useUsers';
import { rangeProblem } from '@/shell/weekly-ranges/model';
import type { Schedule } from '@/types/alerting';

import { MICROS_PER_DAY, buildScheduleTimeline } from './model';
import { UserAvatar, formatScheduleDay, formatScheduleTime } from './Ui';

export const DEFAULT_TZ = 'UTC';

/** How long the editor waits after the last change before asking the server again. */
const PREVIEW_DELAY_MS = 350;

/**
 * The next seven days of a schedule being edited, as the server would cover them: its
 * timezone, daylight saving and minute-precise windows are worked out there, not here.
 */
export function ScheduleDraftPreview({
  schedule,
  users,
  locale,
}: {
  schedule: Schedule;
  users: UserLite[];
  locale: string;
}) {
  const { t } = useTranslation('alerts');
  const byId = React.useMemo(
    () => new Map(users.map((user) => [user.id, user])),
    [users],
  );
  const [now] = React.useState(() => Date.now() * 1000);
  const rotations = schedule.rotations.filter((rotation) => rotation.members.length > 0);
  const ranges = rotations.flatMap((rotation) => rotation.activeWindows ?? []);
  const unreadable = ranges.some((range) => rangeProblem(range) !== null);
  const draft = JSON.stringify({
    timezone: schedule.timezone || DEFAULT_TZ,
    enabled: true,
    rotations,
    overrides: schedule.overrides,
  });
  const [settled, setSettled] = React.useState(draft);

  React.useEffect(() => {
    const timer = window.setTimeout(() => setSettled(draft), PREVIEW_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [draft]);

  const coverage = useQuery({
    queryKey: ['schedule-coverage-preview', settled, now],
    queryFn: () =>
      schedulesApi.previewCoverage({
        ...(JSON.parse(settled) as schedulesApi.CoveragePreviewInput),
        from: now,
        to: now + 7 * MICROS_PER_DAY,
      }),
    enabled: !unreadable,
    placeholderData: keepPreviousData,
    retry: false,
  });
  const segments = React.useMemo(
    () =>
      buildScheduleTimeline({ ...schedule, coverage: coverage.data }, now, 7).slice(0, 7),
    [coverage.data, now, schedule],
  );

  return (
    <aside
      data-schedule-preview
      className="border-t border-bd-0 bg-bg-2 px-5 py-5 xl:overflow-auto xl:border-l xl:border-t-0"
    >
      <div className="flex items-center gap-2">
        <CalendarDays className="h-4 w-4 text-indigo-soft" />
        <h3 className="type-section-title font-sans font-display text-tx-0">
          {t('schedules.preview.title')}
        </h3>
      </div>
      <p className="mt-1 text-xs leading-relaxed text-tx-3">
        {t('schedules.preview.description')}
      </p>

      <div className="mt-4 flex flex-col gap-2">
        {segments.length === 0 ? (
          <div className="bg-bg-1 px-4 py-8 text-center text-xs text-tx-3">
            {unreadable || coverage.isError
              ? t('schedules.preview.unreadable')
              : t('schedules.preview.empty')}
          </div>
        ) : (
          segments.map((segment, index) => {
            const user = segment.userId ? byId.get(segment.userId) : undefined;
            return (
              <div
                key={segment.id}
                className="flex items-center gap-3 border-b border-bd-0 bg-transparent px-1 py-2.5 last:border-b-0"
              >
                <span className="w-20 shrink-0">
                  <span className="block text-xs font-strong text-tx-1">
                    {formatScheduleDay(segment.startAt, locale, schedule.timezone)}
                  </span>
                  <span className="mt-0.5 block font-mono text-type-micro text-tx-3">
                    {formatScheduleTime(segment.startAt, locale, schedule.timezone)}
                  </span>
                </span>
                <UserAvatar user={user} size="sm" />
                <span className="min-w-0 flex-1 truncate text-xs font-strong text-tx-0">
                  {user?.name ?? t('schedules.nobody_on_call')}
                </span>
                {segment.source === 'override' && (
                  <Pill tone="orange">{t('schedules.override_badge')}</Pill>
                )}
                {segment.source === 'gap' && (
                  <Pill tone="red">{t('schedules.status.gap')}</Pill>
                )}
                {index < segments.length - 1 && (
                  <span className="sr-only">{t('schedules.preview.then')}</span>
                )}
              </div>
            );
          })
        )}
      </div>

      <div className="mt-4 grid grid-cols-2 gap-2">
        <div className="bg-bg-1 p-3">
          <UsersRound className="h-4 w-4 text-[var(--blue-ink)]" />
          <div className="mt-2 text-xs text-tx-3">{t('schedules.preview.member_count')}</div>
          <div className="mt-0.5 font-sans text-lg font-display text-tx-0">
            {new Set(schedule.rotations.flatMap((rotation) => rotation.members)).size}
          </div>
        </div>
        <div className="bg-bg-1 p-3">
          <Clock3 className="h-4 w-4 text-[var(--green-ink)]" />
          <div className="mt-2 text-xs text-tx-3">{t('schedules.preview.timezone')}</div>
          <div className="mt-0.5 truncate font-mono text-sm font-strong text-tx-0">
            {schedule.timezone || DEFAULT_TZ}
          </div>
        </div>
      </div>
    </aside>
  );
}
