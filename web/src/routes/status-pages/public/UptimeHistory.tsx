import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

import { STATUS_DOT, componentStatusLabel, statusPageHistoryDays } from '../model';
import { PUBLIC_CONTROL_HEIGHT } from './control';
import type { PublicStatusContext } from './Frame';
import { uptimeHistoryMonths } from './history';
import { MetaLine } from './MetaLine';
import { STATUS_TEXT } from './tone';
import { componentCalendarUptime } from './uptime';
import {
  UptimeDayTooltip,
  UptimeLegend,
  formatUptimePercentage,
  uptimeDayColor,
} from './UptimeDay';

/**
 * How one component has held up, a day to a square, in the months of the
 * calendar. One component at a time: its days are the thing being read, and
 * a wall of every component's calendars would be read by nobody.
 */
export function PublicUptimeHistory({
  context,
  monthKeys,
}: {
  context: PublicStatusContext;
  /** The months being looked at, oldest first. */
  monthKeys: string[];
}) {
  const { snapshot, page, language, copy } = context;
  const [componentId, setComponentId] = React.useState(snapshot.components[0]?.id ?? '');
  const component = snapshot.components.find((candidate) => candidate.id === componentId) ?? snapshot.components[0];

  if (!component) {
    return (
      <p className="mt-8 rounded-xl border border-bd-0 bg-white px-5 py-10 text-center text-sm text-tx-2">
        {copy('public.archive.no_uptime_components')}
      </p>
    );
  }

  const historyDays = statusPageHistoryDays(page);
  const uptime = componentCalendarUptime(component, snapshot, historyDays, page.timezone);
  const visible = new Set(monthKeys);
  const months = uptimeHistoryMonths(uptime, page.timezone, language).filter((month) => visible.has(month.key));
  const dayLabel = new Intl.DateTimeFormat(language, {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    timeZone: page.timezone,
  });
  const weekdays = weekdayInitials(language);

  return (
    <TooltipProvider delayDuration={100}>
      <div className="mt-8 overflow-hidden rounded-2xl border border-bd-0 bg-white">
        <div className="flex flex-col gap-4 border-b border-bd-0 px-5 py-5 sm:flex-row sm:items-center sm:justify-between sm:px-7">
          <Select value={component.id} onValueChange={setComponentId}>
            <SelectTrigger
              aria-label={copy('public.archive.component_label')}
              className={cn(PUBLIC_CONTROL_HEIGHT, 'w-full text-base font-strong text-tx-0 sm:w-72 sm:text-sm')}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent data-theme="light">
              {snapshot.components.map((item) => (
                <SelectItem key={item.id} value={item.id} className="min-h-10">
                  {item.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          {/* The component as it is now, and what the days below add up to. */}
          <MetaLine className="text-sm leading-5 text-tx-2">
            <span className={cn('inline-flex items-center gap-2 font-strong', STATUS_TEXT[component.status])}>
              <span aria-hidden className={cn('h-2.5 w-2.5 rounded-full', STATUS_DOT[component.status])} />
              {componentStatusLabel(copy, component.status)}
            </span>
            <span className="tabular-nums">
              {uptime.percentage === null
                ? copy('public.uptime.value_unavailable')
                : copy('public.archive.window_uptime', {
                    value: formatUptimePercentage(uptime.percentage, language),
                    count: historyDays,
                  })}
            </span>
          </MetaLine>
        </div>

        {/* Each month is a small calendar of its own width; the months stand apart evenly. */}
        <div className="flex flex-wrap justify-between gap-x-10 gap-y-9 px-5 py-7 sm:px-7">
          {months.map((month) => (
            <section key={month.key} aria-labelledby={`uptime-month-${month.key}`} className="w-full sm:w-[17.5rem]">
              <div className="flex items-baseline justify-between gap-3">
                <h3 id={`uptime-month-${month.key}`} className="text-base font-display-strong text-tx-0">
                  {month.label}
                </h3>
                <span className="text-sm tabular-nums text-tx-2">
                  {month.percentage === null
                    ? copy('public.uptime.value_unavailable')
                    : copy('public.archive.month_uptime', {
                        value: formatUptimePercentage(month.percentage, language),
                      })}
                </span>
              </div>

              <div aria-hidden className="mt-3 grid grid-cols-7 gap-1.5 text-center text-xs text-tx-3">
                {weekdays.map((weekday, index) => (
                  <span key={index}>{weekday}</span>
                ))}
              </div>
              <div className="mt-1.5 grid grid-cols-7 gap-1.5" role="grid">
                {Array.from({ length: month.leadingDays }, (_, index) => (
                  <span key={`leading-${index}`} aria-hidden />
                ))}
                {month.days.map((day, index) =>
                  day ? (
                    <Tooltip key={day.startAt}>
                      <TooltipTrigger asChild>
                        <button
                          type="button"
                          role="gridcell"
                          aria-label={copy('public.archive.uptime_day_label', {
                            date: dayLabel.format(new Date(Math.floor(day.startAt / 1_000))),
                            status: day.hasData
                              ? componentStatusLabel(copy, day.status)
                              : copy('public.uptime.no_data_short'),
                          })}
                          className={cn(
                            'aspect-square min-w-0 rounded-[4px] transition-opacity hover:opacity-60 focus-visible:opacity-60',
                            uptimeDayColor(day),
                          )}
                        />
                      </TooltipTrigger>
                      <TooltipContent data-theme="light" side="top" sideOffset={8} className="w-72 p-3 text-tx-1">
                        <UptimeDayTooltip
                          day={day}
                          date={dayLabel.format(new Date(Math.floor(day.startAt / 1_000)))}
                          language={language}
                          copy={copy}
                        />
                      </TooltipContent>
                    </Tooltip>
                  ) : (
                    // A day of the month that lies outside what the page keeps, or has not come yet.
                    <span key={`outside-${index}`} aria-hidden className="aspect-square min-w-0 rounded-[4px] bg-bg-2" />
                  ),
                )}
              </div>
            </section>
          ))}
        </div>

        <div className="border-t border-bd-0 bg-bg-1 px-5 py-3 sm:px-7">
          <UptimeLegend days={uptime.days} copy={copy} />
        </div>
      </div>
    </TooltipProvider>
  );
}

/** The days of the week as a calendar heads its columns, Sunday first as the months are laid out. */
function weekdayInitials(language: string): string[] {
  const format = new Intl.DateTimeFormat(language, { weekday: 'narrow', timeZone: 'UTC' });

  // 4 January 1970 was a Sunday.
  return Array.from({ length: 7 }, (_, index) => format.format(new Date(Date.UTC(1970, 0, 4 + index))));
}
