import type { TFunction } from 'i18next';
import { ChevronDown, Info } from 'lucide-react';
import * as React from 'react';

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

import { STATUS_DOT, componentStatusLabel, componentTree, groupStatus } from '../model';
import { STATUS_TEXT } from './tone';
import { type ComponentUptime, componentUptime, groupUptime } from './uptime';
import {
  UptimeDayTooltip,
  formatUptimePercentage,
  uptimeDayColor,
} from './UptimeDay';

interface StripContext {
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}

/**
 * Every component of the service: how it is now, and how it has held up day
 * by day over ninety days. Components in a group are listed under it, the way
 * files are in a folder; a group shows the worst of them.
 *
 * Each strip carries its own scale under it, the way a reader expects from a
 * status page: where it begins, how much of it the component was up, where it
 * ends. What a day's colour means is said by the day itself, on hover.
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
  const entries = componentTree(snapshot.components);
  const context = { timezone, language, copy };

  if (entries.length === 0) {
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
          {entries.map(({ item, members }) =>
            item.kind === 'group' ? (
              <ComponentGroup
                key={item.id}
                group={item}
                members={members.map((member) => ({ component: member, uptime: componentUptime(member, snapshot) }))}
                context={context}
              />
            ) : (
              <li key={item.id} data-status-component={item.status} className="px-5 py-5 sm:px-6">
                <ComponentRow component={item} uptime={componentUptime(item, snapshot)} context={context} />
              </li>
            ))}
        </ul>
      </div>
    </TooltipProvider>
  );
}

/**
 * A group of components. Folded, it is one row with the worst status of its
 * components and their strips laid over each other; unfolded, the components
 * themselves. It opens unfolded when one of them is not operational.
 */
function ComponentGroup({
  group,
  members,
  context,
}: {
  group: PublicStatusPageComponent;
  members: Array<{ component: PublicStatusPageComponent; uptime: ComponentUptime }>;
  context: StripContext;
}) {
  const { copy } = context;
  const status = groupStatus(members.map(({ component }) => component));
  const [open, setOpen] = React.useState(status !== 'operational');
  const listId = React.useId();

  return (
    <li data-status-component={status} data-status-group className="px-5 py-5 sm:px-6">
      <div className="flex items-start gap-3">
        {!open && <StatusDot status={status} />}
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-2 gap-y-1">
          <h3 className="min-w-0 text-base font-bold leading-6 text-tx-0">{group.name}</h3>
          {group.description && <ComponentDescription name={group.name} description={group.description} copy={copy} />}
          <button
            type="button"
            aria-expanded={open}
            aria-controls={listId}
            onClick={() => setOpen((current) => !current)}
            className="inline-flex min-h-[32px] items-center gap-1 rounded-md px-1.5 text-sm text-tx-2 transition-colors hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0"
          >
            {copy('public.group_components', { count: members.length })}
            <ChevronDown aria-hidden className={cn('h-4 w-4 transition-transform', open && 'rotate-180')} />
          </button>
        </div>
        {!open && <StatusText status={status} copy={copy} />}
      </div>

      {open ? (
        <ul id={listId} className="mt-4 space-y-5 border-l border-bd-0 pl-4 sm:pl-5">
          {members.map(({ component, uptime }) => (
            <li key={component.id} data-status-component={component.status}>
              <ComponentRow component={component} uptime={uptime} context={context} />
            </li>
          ))}
        </ul>
      ) : (
        <UptimeStrip name={group.name} uptime={groupUptime(members.map(({ uptime }) => uptime))} context={context} />
      )}
    </li>
  );
}

function ComponentRow({
  component,
  uptime,
  context,
}: {
  component: PublicStatusPageComponent;
  uptime: ComponentUptime;
  context: StripContext;
}) {
  const { copy } = context;

  return (
    <>
      <div className="flex items-start gap-3">
        <StatusDot status={component.status} />
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <h3 className="min-w-0 text-base font-bold leading-6 text-tx-0">{component.name}</h3>
          {component.description && (
            <ComponentDescription name={component.name} description={component.description} copy={copy} />
          )}
        </div>
        <StatusText status={component.status} copy={copy} />
      </div>
      <UptimeStrip name={component.name} uptime={uptime} context={context} />
    </>
  );
}

function StatusDot({ status }: { status: ComponentStatus }) {
  return <span aria-hidden className={cn('mt-[7px] h-2.5 w-2.5 shrink-0 rounded-full', STATUS_DOT[status])} />;
}

function StatusText({ status, copy }: { status: ComponentStatus; copy: TFunction }) {
  return (
    <p className={cn('shrink-0 text-right text-sm font-strong leading-6', STATUS_TEXT[status])}>
      {componentStatusLabel(copy, status)}
    </p>
  );
}

/** Ninety days as one strip, with its own scale under it. */
function UptimeStrip({ name, uptime, context }: { name: string; uptime: ComponentUptime; context: StripContext }) {
  const { timezone, language, copy } = context;
  const percentage = uptime.percentage === null ? null : formatUptimePercentage(uptime.percentage, language);
  const dayLabel = new Intl.DateTimeFormat(language, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    timeZone: timezone,
  });

  return (
    <>
      <div
        role="img"
        aria-label={percentage === null
          ? copy('public.uptime.chart_label_unavailable', { name })
          : copy('public.uptime.chart_label', { name, value: percentage })}
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

      {/* The strip's own scale: where it begins, how much of it the component was up, where it ends. */}
      <div data-uptime-scale className="mt-2.5 flex items-center gap-3 text-xs leading-5 text-tx-2">
        <span className="shrink-0">{copy('public.uptime.days_ago')}</span>
        <span aria-hidden className="h-px min-w-3 flex-1 bg-bd-1" />
        <span className="shrink-0 tabular-nums">
          {percentage === null
            ? copy('public.uptime.value_unavailable')
            : copy('public.uptime.value', { value: percentage })}
        </span>
        <span aria-hidden className="h-px min-w-3 flex-1 bg-bd-1" />
        <span className="shrink-0">{copy('public.uptime.today')}</span>
      </div>
    </>
  );
}

/**
 * What a component is, in the publisher's words, behind a sign beside its
 * name: the list reads as names, and the words are a hover, a focus or a tap
 * away. A phone has no hover, so a tap opens it and a tap elsewhere closes it.
 */
function ComponentDescription({
  name,
  description,
  copy,
}: {
  name: string;
  description: string;
  copy: TFunction;
}) {
  const [open, setOpen] = React.useState(false);

  return (
    <Tooltip open={open} onOpenChange={setOpen}>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={copy('public.component_description', { name })}
          onPointerDown={(event) => {
            // A tap is not a click that should close it again.
            if (event.pointerType === 'touch') event.preventDefault();
          }}
          onClick={(event) => {
            event.preventDefault();
            setOpen(true);
          }}
          className="grid h-6 w-6 shrink-0 place-items-center rounded-md bg-bg-2 text-tx-2 transition-colors hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
        >
          <Info aria-hidden className="h-3.5 w-3.5" />
        </button>
      </TooltipTrigger>
      <TooltipContent
        data-theme="light"
        side="top"
        align="start"
        sideOffset={8}
        className="max-w-[300px] rounded-xl border border-bd-1 bg-white px-4 py-3 text-sm leading-6 text-tx-1"
      >
        {description}
      </TooltipContent>
    </Tooltip>
  );
}
