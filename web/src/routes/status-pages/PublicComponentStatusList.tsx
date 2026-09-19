import type { TFunction } from 'i18next';

import type {
  ComponentStatus,
  PublicStatusPageComponent,
  PublicStatusPageSnapshot,
  StatusPageLanguage,
} from '@/api/statusPages';
import { cn } from '@/shell/lib/cn';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

import { componentUptime, type ComponentUptime } from './componentUptime';
import { STATUS_DOT, componentStatusLabel } from './model';

const COMPONENT_STATUS_TEXT: Record<ComponentStatus, string> = {
  operational: 'text-green-soft',
  degraded_performance: 'text-yellow-soft',
  partial_outage: 'text-orange-soft',
  major_outage: 'text-red-soft',
  maintenance: 'text-blue-soft',
};

export function PublicComponentStatusList({
  snapshot,
  timezone,
  language,
  empty,
  copy,
}: {
  snapshot: PublicStatusPageSnapshot;
  timezone: string;
  language: StatusPageLanguage;
  empty: string;
  copy: TFunction;
}) {
  return (
    <div className="mt-5 overflow-hidden rounded-xl border border-bd-0 bg-white">
      {snapshot.components.length === 0 ? (
        <p className="px-5 py-10 text-center text-sm text-tx-3">{empty}</p>
      ) : (
        snapshot.components.map((component) => (
          <PublicComponentRow
            key={component.id}
            component={component}
            uptime={componentUptime(component, snapshot)}
            timezone={timezone}
            language={language}
            copy={copy}
          />
        ))
      )}
    </div>
  );
}

function PublicComponentRow({
  component,
  uptime,
  timezone,
  language,
  copy,
}: {
  component: PublicStatusPageComponent;
  uptime: ComponentUptime;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  return (
    <div className="border-b border-bd-0 px-5 py-5 last:border-b-0 sm:px-6 sm:py-6">
      <div className="flex items-start gap-4">
        <span
          aria-hidden
          className={cn(
            'mt-1.5 h-2.5 w-2.5 shrink-0 rounded-full',
            STATUS_DOT[component.status],
          )}
        />
        <div className="min-w-0 flex-1">
          <h3 className="text-base font-bold text-tx-0">{component.name}</h3>
          {component.description && (
            <p className="mt-1 text-sm leading-5 text-tx-2">{component.description}</p>
          )}
        </div>
        <span
          className={cn(
            'shrink-0 pt-0.5 text-right text-sm font-strong',
            COMPONENT_STATUS_TEXT[component.status],
          )}
        >
          {componentStatusLabel(copy, component.status)}
        </span>
      </div>
      <ComponentUptimeChart
        component={component}
        uptime={uptime}
        timezone={timezone}
        language={language}
        copy={copy}
      />
    </div>
  );
}

function ComponentUptimeChart({
  component,
  uptime,
  timezone,
  language,
  copy,
}: {
  component: PublicStatusPageComponent;
  uptime: ComponentUptime;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  const percentage = uptime.percentage === null
    ? null
    : new Intl.NumberFormat(language, {
        minimumFractionDigits: uptime.percentage >= 99.995 ? 0 : 2,
        maximumFractionDigits: 2,
      }).format(uptime.percentage);
  const dateFormatter = new Intl.DateTimeFormat(language, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    timeZone: timezone,
  });

  return (
    <TooltipProvider delayDuration={100}>
      <div className="mt-4">
        <div
          role="img"
          aria-label={percentage === null
            ? copy('public.uptime.chart_label_unavailable', { name: component.name })
            : copy('public.uptime.chart_label', {
                name: component.name,
                value: percentage,
              })}
          className="grid h-8 gap-px sm:h-9 sm:gap-1"
          style={{ gridTemplateColumns: `repeat(${uptime.days.length}, minmax(1px, 1fr))` }}
        >
          {uptime.days.map((day) => (
            <Tooltip key={day.startAt}>
              <TooltipTrigger asChild>
                <span
                  aria-hidden
                  className={cn(
                    'min-w-px cursor-default rounded-[1px] transition-opacity hover:opacity-75',
                    day.hasData ? STATUS_DOT[day.status] : 'bg-bd-2',
                  )}
                />
              </TooltipTrigger>
              <TooltipContent
                data-theme="light"
                side="top"
                sideOffset={8}
                className="w-72 p-3 text-tx-1"
              >
                <UptimeDayTooltip
                  day={day}
                  date={dateFormatter.format(new Date(day.startAt / 1_000))}
                  language={language}
                  copy={copy}
                />
              </TooltipContent>
            </Tooltip>
          ))}
        </div>
        <div className="mt-2 flex items-center gap-2 text-xs text-tx-3">
          <span className="shrink-0">{copy('public.uptime.days_ago')}</span>
          <span aria-hidden className="h-px min-w-3 flex-1 bg-bd-1" />
          <span className="shrink-0 font-strong tabular-nums text-tx-2">
            {percentage === null
              ? copy('public.uptime.value_unavailable')
              : copy('public.uptime.value', { value: percentage })}
          </span>
          <span aria-hidden className="h-px min-w-3 flex-1 bg-bd-1" />
          <span className="shrink-0">{copy('public.uptime.today')}</span>
        </div>
      </div>
    </TooltipProvider>
  );
}

export function UptimeDayTooltip({
  day,
  date,
  language,
  copy,
}: {
  day: ComponentUptime['days'][number];
  date: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  if (!day.hasData) {
    return (
      <div>
        <p className="text-sm font-bold text-tx-0">{date}</p>
        <p className="mt-3 text-sm leading-5 text-tx-2">
          {copy('public.uptime.no_data')}
        </p>
      </div>
    );
  }
  const isOperational = day.status === 'operational';
  return (
    <div>
      <p className="text-sm font-bold text-tx-0">{date}</p>
      <div className="mt-3 flex items-center justify-between gap-3 rounded-md bg-bg-1 px-3 py-2">
        <span className="inline-flex min-w-0 items-center gap-2 text-sm font-strong text-tx-1">
          <span
            aria-hidden
            className={cn('h-2.5 w-2.5 shrink-0 rounded-full', STATUS_DOT[day.status])}
          />
          {componentStatusLabel(copy, day.status)}
        </span>
        {!isOperational && (
          <span className="shrink-0 text-xs tabular-nums text-tx-2">
            {day.downtimeMicros > 0
              ? formatDuration(day.downtimeMicros, language, copy)
              : copy('public.uptime.duration_unavailable')}
          </span>
        )}
      </div>

      {isOperational && day.incidents.length === 0 ? (
        <p className="mt-3 text-sm leading-5 text-tx-2">
          {copy('public.uptime.no_downtime')}
        </p>
      ) : day.incidents.length > 0 ? (
        <div className="mt-3">
          <p className="text-xs font-strong uppercase tracking-[0.08em] text-tx-3">
            {copy('public.uptime.related')}
          </p>
          <ul className="mt-2 space-y-1.5 text-sm leading-5 text-tx-1">
            {day.incidents.map((incident) => (
              <li key={incident.id}>{incident.title}</li>
            ))}
          </ul>
        </div>
      ) : (
        <p className="mt-3 text-sm leading-5 text-tx-2">
          {copy('public.uptime.no_incident_details')}
        </p>
      )}
    </div>
  );
}

function formatDuration(
  micros: number,
  language: StatusPageLanguage,
  copy: TFunction,
): string {
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
  if (hours > 0) {
    return copy('public.uptime.duration_hours', { hours: formatter.format(hours) });
  }
  return copy('public.uptime.duration_minutes', {
    minutes: formatter.format(minutes),
  });
}
