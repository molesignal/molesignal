import type { TFunction } from 'i18next';

import type {
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

import { STATUS_DOT, componentStatusLabel } from '../model';
import { STATUS_TEXT } from './tone';
import { type ComponentUptime, componentUptime } from './uptime';
import {
  UptimeDayTooltip,
  UptimeLegend,
  formatUptimePercentage,
  uptimeDayColor,
} from './UptimeDay';

/**
 * Every component of the service: how it is now, and how it has held up day
 * by day over ninety days.
 *
 * The strips share one scale, so it is written once, under all of them: when
 * the strip begins and ends, and what its colours mean.
 */
export function PublicComponentList({
  snapshot,
  timezone,
  language,
  copy,
}: {
  snapshot: PublicStatusPageSnapshot;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  const rows = snapshot.components.map((component) => ({
    component,
    uptime: componentUptime(component, snapshot),
  }));

  if (rows.length === 0) {
    return (
      <p className="mt-5 rounded-xl border border-bd-0 bg-white px-5 py-10 text-center text-sm text-tx-2">
        {copy('public.no_components')}
      </p>
    );
  }

  return (
    <TooltipProvider delayDuration={100}>
      <div className="mt-5 overflow-hidden rounded-xl border border-bd-0 bg-white">
        <ul className="divide-y divide-bd-0">
          {rows.map(({ component, uptime }) => (
            <ComponentRow
              key={component.id}
              component={component}
              uptime={uptime}
              timezone={timezone}
              language={language}
              copy={copy}
            />
          ))}
        </ul>
        <div className="grid grid-cols-2 gap-y-2.5 border-t border-bd-0 bg-bg-1 px-5 py-3 text-xs text-tx-2 sm:grid-cols-[auto_minmax(0,1fr)_auto] sm:items-center sm:gap-x-6 sm:px-6">
          <span>{copy('public.uptime.days_ago')}</span>
          <span className="text-right sm:order-3">{copy('public.uptime.today')}</span>
          <UptimeLegend
            days={rows.flatMap(({ uptime }) => uptime.days)}
            copy={copy}
            className="col-span-2 sm:order-2 sm:col-span-1 sm:justify-center"
          />
        </div>
      </div>
    </TooltipProvider>
  );
}

function ComponentRow({
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
  const percentage = uptime.percentage === null ? null : formatUptimePercentage(uptime.percentage, language);
  const dayLabel = new Intl.DateTimeFormat(language, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    timeZone: timezone,
  });

  return (
    <li data-status-component={component.status} className="px-5 py-5 sm:px-6">
      <div className="flex items-start gap-3">
        <span aria-hidden className={cn('mt-[7px] h-2.5 w-2.5 shrink-0 rounded-full', STATUS_DOT[component.status])} />
        <div className="min-w-0 flex-1">
          <h3 className="text-base font-bold leading-6 text-tx-0">{component.name}</h3>
          {component.description && (
            <p className="mt-0.5 text-pretty text-sm leading-5 text-tx-2">{component.description}</p>
          )}
        </div>
        <div className="shrink-0 text-right">
          <p className={cn('text-sm font-strong leading-6', STATUS_TEXT[component.status])}>
            {componentStatusLabel(copy, component.status)}
          </p>
          <p className="mt-0.5 text-xs tabular-nums leading-5 text-tx-2">
            {percentage === null
              ? copy('public.uptime.value_unavailable')
              : copy('public.uptime.value', { value: percentage })}
          </p>
        </div>
      </div>

      <div
        role="img"
        aria-label={percentage === null
          ? copy('public.uptime.chart_label_unavailable', { name: component.name })
          : copy('public.uptime.chart_label', { name: component.name, value: percentage })}
        className="mt-4 grid h-8 gap-px sm:h-9 sm:gap-[3px]"
        style={{ gridTemplateColumns: `repeat(${uptime.days.length}, minmax(1px, 1fr))` }}
      >
        {uptime.days.map((day) => (
          <Tooltip key={day.startAt}>
            <TooltipTrigger asChild>
              <span
                aria-hidden
                className={cn(
                  'min-w-px cursor-default rounded-[1px] transition-opacity hover:opacity-60 sm:rounded-[2px]',
                  uptimeDayColor(day),
                )}
              />
            </TooltipTrigger>
            <TooltipContent data-theme="light" side="top" sideOffset={8} className="w-72 p-3 text-tx-1">
              <UptimeDayTooltip
                day={day}
                date={dayLabel.format(new Date(day.startAt / 1_000))}
                language={language}
                copy={copy}
              />
            </TooltipContent>
          </Tooltip>
        ))}
      </div>
    </li>
  );
}
