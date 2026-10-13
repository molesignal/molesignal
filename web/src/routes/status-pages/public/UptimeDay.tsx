import type { TFunction } from 'i18next';

import type { ComponentStatus, StatusPageLanguage } from '@/api/statusPages';
import { cn } from '@/shell/lib/cn';

import { COMPONENT_STATUSES, STATUS_DOT, componentStatusLabel } from '../model';
import { UPTIME_DAY, UPTIME_DAY_UNKNOWN } from './tone';
import type { ComponentUptimeDay } from './uptime';

/** The colour of one day in an uptime strip or calendar; a day nothing was recorded on is grey. */
export function uptimeDayColor(day: ComponentUptimeDay): string {
  return day.hasData ? UPTIME_DAY[day.status] : UPTIME_DAY_UNKNOWN;
}

/** An uptime percentage as it is written: `100`, or two decimals once anything was lost. */
export function formatUptimePercentage(value: number, language: StatusPageLanguage): string {
  return new Intl.NumberFormat(language, {
    minimumFractionDigits: value >= 99.995 ? 0 : 2,
    maximumFractionDigits: 2,
  }).format(value);
}

/**
 * What the colours of the days mean. It names only the states that occur in
 * the days being shown, so a page that has only ever been up is not given a
 * key to four colours it does not use.
 */
export function UptimeLegend({
  days,
  copy,
  className,
}: {
  days: ComponentUptimeDay[];
  copy: TFunction;
  className?: string;
}) {
  const seen = new Set<ComponentStatus>(days.filter((day) => day.hasData).map((day) => day.status));
  const statuses = COMPONENT_STATUSES.filter((status) => status === 'operational' || seen.has(status));
  const hasGaps = days.some((day) => !day.hasData);

  return (
    <ul
      aria-label={copy('public.uptime.legend')}
      className={cn('flex flex-wrap items-center gap-x-4 gap-y-1.5 text-xs text-tx-2', className)}
    >
      {statuses.map((status) => (
        <li key={status} className="inline-flex items-center gap-1.5">
          <span aria-hidden className={cn('h-2.5 w-2.5 rounded-[2px]', UPTIME_DAY[status])} />
          {componentStatusLabel(copy, status)}
        </li>
      ))}
      {hasGaps && (
        <li className="inline-flex items-center gap-1.5">
          <span aria-hidden className={cn('h-2.5 w-2.5 rounded-[2px]', UPTIME_DAY_UNKNOWN)} />
          {copy('public.uptime.no_data_short')}
        </li>
      )}
    </ul>
  );
}

/** What is known of one day: its state, how long it was not well, and the incidents behind it. */
export function UptimeDayTooltip({
  day,
  date,
  language,
  copy,
}: {
  day: ComponentUptimeDay;
  date: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  if (!day.hasData) {
    return (
      <div>
        <p className="text-sm font-bold text-tx-0">{date}</p>
        <p className="mt-3 text-sm leading-5 text-tx-2">{copy('public.uptime.no_data')}</p>
      </div>
    );
  }

  const isOperational = day.status === 'operational';

  return (
    <div>
      <p className="text-sm font-bold text-tx-0">{date}</p>
      <div className="mt-3 flex items-center justify-between gap-3 rounded-md bg-bg-1 px-3 py-2">
        <span className="inline-flex min-w-0 items-center gap-2 text-sm font-strong text-tx-1">
          <span aria-hidden className={cn('h-2.5 w-2.5 shrink-0 rounded-full', STATUS_DOT[day.status])} />
          {componentStatusLabel(copy, day.status)}
        </span>
        {!isOperational && (
          <span className="shrink-0 text-xs tabular-nums text-tx-2">
            {day.downtimeMicros > 0
              ? formatDowntime(day.downtimeMicros, language, copy)
              : copy('public.uptime.duration_unavailable')}
          </span>
        )}
      </div>

      {isOperational && day.incidents.length === 0 ? (
        <p className="mt-3 text-sm leading-5 text-tx-2">{copy('public.uptime.no_downtime')}</p>
      ) : day.incidents.length > 0 ? (
        <div className="mt-3">
          <p className="text-xs font-strong text-tx-3">{copy('public.uptime.related')}</p>
          <ul className="mt-2 space-y-1.5 text-sm leading-5 text-tx-1">
            {day.incidents.map((incident) => (
              <li key={incident.id}>{incident.title}</li>
            ))}
          </ul>
        </div>
      ) : (
        <p className="mt-3 text-sm leading-5 text-tx-2">{copy('public.uptime.no_incident_details')}</p>
      )}
    </div>
  );
}

function formatDowntime(micros: number, language: StatusPageLanguage, copy: TFunction): string {
  const totalMinutes = Math.max(1, Math.round(micros / (60 * 1_000_000)));
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  const formatter = new Intl.NumberFormat(language);

  if (hours > 0 && minutes > 0) {
    return copy('public.uptime.duration_hours_minutes', {
      hours: formatter.format(hours),
      minutes: formatter.format(minutes),
    });
  }
  if (hours > 0) return copy('public.uptime.duration_hours', { hours: formatter.format(hours) });

  return copy('public.uptime.duration_minutes', { minutes: formatter.format(minutes) });
}
