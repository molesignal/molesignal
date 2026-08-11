import type { TFunction } from 'i18next';
import * as React from 'react';

import type {
  PublicStatusPageSnapshot,
  StatusPageLanguage,
} from '@/api/statusPages';
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

import { componentCalendarUptime } from './componentUptime';
import {
  componentStatusLabel,
  STATUS_DOT,
  statusPageHistoryDays,
} from './model';
import { UptimeDayTooltip } from './PublicComponentStatusList';
import { uptimeHistoryMonths } from './publicStatusHistory';

export function PublicUptimeHistory({
  snapshot,
  timezone,
  language,
  monthKeys,
  copy,
}: {
  snapshot: PublicStatusPageSnapshot;
  timezone: string;
  language: StatusPageLanguage;
  monthKeys: string[];
  copy: TFunction;
}) {
  const [componentId, setComponentId] = React.useState(
    snapshot.components[0]?.id ?? '',
  );
  const historyDays = statusPageHistoryDays(snapshot.page);
  const component =
    snapshot.components.find((candidate) => candidate.id === componentId)
    ?? snapshot.components[0];

  React.useEffect(() => {
    if (component && component.id !== componentId) setComponentId(component.id);
  }, [component, componentId]);

  const visibleMonthKeys = new Set(monthKeys);
  const months = component
    ? uptimeHistoryMonths(
        componentCalendarUptime(component, snapshot, historyDays, timezone),
        timezone,
        language,
      ).filter((month) => visibleMonthKeys.has(month.key))
    : [];
  const dateFormatter = new Intl.DateTimeFormat(language, {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    timeZone: timezone,
  });

  return (
    <section className="mt-8 sm:mt-10" aria-labelledby="uptime-history-heading">
      <header className="border-b border-bd-0 pb-5">
        <h2
          id="uptime-history-heading"
          className="text-2xl font-display-strong tracking-[-0.02em] text-tx-0 sm:text-3xl"
        >
          {copy('public.archive.uptime_title')}
        </h2>
        <p className="mt-2 text-sm leading-6 text-tx-2">
          {copy('public.archive.uptime_description', { count: historyDays })}
        </p>
      </header>

      {snapshot.components.length === 0 ? (
        <p className="border-b border-bd-0 py-12 text-center text-sm text-tx-2">
          {copy('public.archive.no_uptime_components')}
        </p>
      ) : (
        <>
          <div className="mt-7 max-w-sm">
            <label className="block text-sm font-strong text-tx-2">
              <span>{copy('public.archive.component_label')}</span>
              <Select value={componentId} onValueChange={setComponentId}>
                <SelectTrigger
                  aria-label={copy('public.archive.component_label')}
                  className="mt-2 h-11 w-full bg-white text-base font-strong text-tx-1 sm:text-sm"
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
            </label>
          </div>

          <TooltipProvider delayDuration={100}>
            <div className="mt-9 grid gap-x-8 gap-y-12 md:grid-cols-2 xl:grid-cols-3">
              {months.map((month) => (
                <section key={month.key} aria-labelledby={`uptime-month-${month.key}`}>
                  <div className="flex items-center justify-between gap-3 border-b border-bd-0 pb-2.5">
                    <h3
                      id={`uptime-month-${month.key}`}
                      className="text-lg font-display-strong text-tx-0"
                    >
                      {month.label}
                    </h3>
                    <span className="text-sm font-strong tabular-nums text-tx-2">
                      {month.percentage === null
                        ? copy('public.uptime.value_unavailable')
                        : copy('public.archive.month_uptime', {
                            value: formatPercentage(month.percentage, language),
                          })}
                    </span>
                  </div>

                  <div className="mt-3 grid grid-cols-7 gap-1" role="grid">
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
                                date: dateFormatter.format(
                                  new Date(Math.floor(day.startAt / 1_000)),
                                ),
                                status: day.hasData
                                  ? componentStatusLabel(copy, day.status)
                                  : copy('public.uptime.no_data_short'),
                              })}
                              className={cn(
                                'aspect-square min-w-0 rounded-[2px] transition-opacity hover:opacity-75 focus-visible:opacity-75',
                                day.hasData ? STATUS_DOT[day.status] : 'bg-bd-2',
                              )}
                            />
                          </TooltipTrigger>
                          <TooltipContent
                            data-theme="light"
                            side="top"
                            sideOffset={8}
                            className="w-72 border-bd-1 bg-white p-3 text-tx-1"
                          >
                            <UptimeDayTooltip
                              day={day}
                              date={dateFormatter.format(
                                new Date(Math.floor(day.startAt / 1_000)),
                              )}
                              language={language}
                              copy={copy}
                            />
                          </TooltipContent>
                        </Tooltip>
                      ) : (
                        <span
                          key={`empty-${index}`}
                          aria-hidden
                          className="aspect-square min-w-0 rounded-[2px] bg-bg-2"
                        />
                      ),
                    )}
                  </div>
                </section>
              ))}
            </div>
          </TooltipProvider>
        </>
      )}
    </section>
  );
}

function formatPercentage(value: number, language: StatusPageLanguage): string {
  return new Intl.NumberFormat(language, {
    minimumFractionDigits: value >= 99.995 ? 0 : 2,
    maximumFractionDigits: 2,
  }).format(value);
}
