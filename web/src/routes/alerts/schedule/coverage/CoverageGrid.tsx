import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import type { UserLite } from '@/shell/useUsers';
import type { Schedule } from '@/types/alerting';

import { formatScheduleDateTime, formatScheduleDay, ScheduleCard } from '../Ui';
import { coverageDays, coverageGaps, gapHours, type CoverageCell } from './model';

/** The server works coverage out for 14 days ahead. */
const DAYS_AHEAD = 14;
const DAY_MICROS = 86_400_000_000;

const CELL_FILL: Record<NonNullable<CoverageCell['source']> | 'none', string> = {
  rotation: 'bg-[var(--indigo-tint)]',
  override: 'bg-[var(--blue-fill)]',
  gap: 'bg-[var(--red-fill)]',
  none: 'bg-bg-2',
};

const HOURS_MARKED = [0, 6, 12, 18];

/**
 * Who stands behind each hour of the next two weeks: a glance finds the holes,
 * and each hole is listed with the way to fill it. The rotations and overrides
 * are read on the server, so what this shows is what paging will do.
 */
export function CoverageGrid({
  schedule,
  users,
  nowMicros,
  locale,
  onAddOverride,
}: {
  schedule: Schedule;
  users: readonly UserLite[];
  nowMicros: number;
  locale: string;
  /** Absent for someone who cannot change the schedule. */
  onAddOverride?: (() => void) | undefined;
}) {
  const { t } = useTranslation('alerts');
  const segments = React.useMemo(() => schedule.coverage?.segments ?? [], [schedule.coverage]);
  const days = React.useMemo(
    () => coverageDays(segments, nowMicros, DAYS_AHEAD, schedule.timezone),
    [nowMicros, schedule.timezone, segments],
  );
  const gaps = React.useMemo(
    () => coverageGaps(segments, nowMicros, nowMicros + DAYS_AHEAD * DAY_MICROS),
    [nowMicros, segments],
  );
  const nameOf = (id: string | undefined) => users.find((user) => user.id === id)?.name ?? t('schedules.unknown_user');
  const describe = (cell: CoverageCell) => {
    const when = formatScheduleDateTime(cell.start, locale, schedule.timezone);
    if (cell.source === 'gap' || cell.source === null) return `${when} · ${t('schedules.coverage.nobody')}`;
    if (cell.source === 'override') return `${when} · ${nameOf(cell.userId)} · ${t('schedules.coverage.legend.override')}`;
    return `${when} · ${nameOf(cell.userId)}`;
  };

  return (
    <ScheduleCard title={t('schedules.coverage.title')}>
      <p data-coverage-summary className={cn('pt-3 font-sans text-sm', gaps.length > 0 ? 'text-[var(--red-ink)]' : 'text-tx-1')}>
        {gaps.length > 0
          ? t('schedules.coverage.gaps', { count: gaps.length, hours: gapHours(gaps), days: DAYS_AHEAD })
          : t('schedules.coverage.no_gaps', { days: DAYS_AHEAD })}
      </p>

      <div data-coverage-grid className="mt-3 grid grid-cols-[88px_repeat(24,minmax(0,1fr))] gap-0.5 font-sans">
        <span />
        {Array.from({ length: 24 }, (_, hour) => (
          <span key={hour} className="text-type-micro tabular-nums text-tx-3">
            {HOURS_MARKED.includes(hour) ? String(hour).padStart(2, '0') : ''}
          </span>
        ))}
        {days.map((day) => (
          <React.Fragment key={day.start}>
            <span className="truncate pr-2 text-xs tabular-nums text-tx-2">
              {formatScheduleDay(day.start, locale, schedule.timezone)}
            </span>
            {day.cells.map((cell) => {
              const past = cell.end <= nowMicros;
              const current = cell.start <= nowMicros && nowMicros < cell.end;

              return (
                <span
                  key={cell.start}
                  title={describe(cell)}
                  data-coverage-cell={cell.source ?? 'none'}
                  className={cn(
                    'h-4 rounded-[2px]',
                    CELL_FILL[cell.source ?? 'none'],
                    past && 'opacity-40',
                    current && 'shadow-[inset_0_0_0_1.5px_var(--tx-0)]',
                  )}
                />
              );
            })}
          </React.Fragment>
        ))}
      </div>

      <ul className="mt-2 flex flex-wrap gap-x-4 gap-y-1 font-sans text-xs text-tx-3">
        {(['rotation', 'override', 'gap'] as const).map((source) => (
          <li key={source} className="flex items-center gap-1.5">
            <span aria-hidden className={cn('h-2.5 w-2.5 rounded-sm', CELL_FILL[source])} />
            {t(`schedules.coverage.legend.${source}`)}
          </li>
        ))}
      </ul>

      {gaps.length > 0 && (
        <ul data-coverage-gaps className="mt-3 border-t border-bd-0 font-sans text-sm">
          {gaps.map((gap) => (
            <li key={gap.from} className="flex min-h-9 items-center gap-3 border-b border-bd-0 py-1.5 last:border-b-0">
              <span className="min-w-0 flex-1 tabular-nums text-tx-1">
                {formatScheduleDateTime(gap.from, locale, schedule.timezone)}
                {' → '}
                {formatScheduleDateTime(gap.to, locale, schedule.timezone)}
              </span>
              <span className="shrink-0 tabular-nums text-tx-2">
                {t('schedules.coverage.hours', { hours: gapHours([gap]) })}
              </span>
              {onAddOverride && (
                <ChromeButton size="sm" onClick={onAddOverride}>
                  {t('schedules.add_override')}
                </ChromeButton>
              )}
            </li>
          ))}
        </ul>
      )}
    </ScheduleCard>
  );
}
