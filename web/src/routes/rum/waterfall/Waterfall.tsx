import { useQuery } from '@tanstack/react-query';
import { RefreshCw } from 'lucide-react';
import * as React from 'react';
import { Trans, useTranslation } from 'react-i18next';


import * as rumApi from '@/api/rum';
import type { RumRow } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { Skeleton } from '@/shell/ui/skeleton';
import { useAuthStore } from '@/stores/auth';
import { useMeasuredWidth } from '@/viz/trace/waterfall/Axis';

import { ResourceDetail, TaskDetail } from './Detail';
import { typeInk, statusInk } from './ink';
import {
  type Axis,
  axisFor,
  drawn,
  FIRST_ROWS,
  focusQuery,
  type MarkKey,
  markOf,
  mostCameInUnder,
  placeLabels,
  readResources,
  readTasks,
  SHOW_FILTERS,
  type ShowFilter,
  shareOf,
  typesOf,
  type ViewFacts,
  viewFacts,
  type WaterfallResource,
  type WaterfallTask,
} from './model';
import { useViewReads } from './reads';
import { formatDurationMs } from '../_helpers';
import { gradeOf, VITALS } from '../overview/model';
import { GRADE_TEXT } from '../performance/ink';
import { FIGURE } from '../performance/Reading';

const LCP_LEVEL = VITALS[0] as (typeof VITALS)[number];

/** The label column and the track of every row, so that the rows, the axis and the lines over them stand on the same columns. */
const COLUMNS = 'grid grid-cols-[minmax(150px,280px)_minmax(0,1fr)] gap-x-3 px-4';

const CONTROL = 'h-7 border-0 bg-[var(--control-surface)] px-2.5 text-xs hover:bg-bg-3 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3';

/** The room over the rows for the numbers of the axis and the names of the moments. */
const HEAD_HEIGHT = 76;

/** Where the name of a moment stands: three lines, so that moments close together do not cover each other. */
const MARK_TOP = [22, 38, 54] as const;

const MARK_LINE: Record<MarkKey, string> = {
  first_byte: 'var(--chart-1)',
  fcp: 'var(--chart-2)',
  dcl: 'var(--chart-7)',
  lcp: 'var(--yellow)',
  load: 'var(--tx-4)',
};

const MARK_INK: Record<Exclude<MarkKey, 'lcp'>, string> = {
  first_byte: 'text-[var(--chart-1)]',
  fcp: 'text-[var(--chart-2)]',
  dcl: 'text-[var(--chart-7)]',
  load: 'text-tx-1',
};

/** A tick of the axis: whole seconds from a second on, else milliseconds. */
const tickLabel = (ms: number): string => (ms === 0 ? '0' : ms >= 1_000 ? `${Number((ms / 1_000).toFixed(2))} s` : `${ms} ms`);

type Pending = { isPending: boolean; isError: boolean; refetch: () => unknown };

/** What stands in the place of the rows until they are there, or when they cannot be. */
function ReadState({ read }: { read: Pending }) {
  const { t } = useTranslation('rum');

  if (read.isPending) {
    return (
      <div role="status" aria-label={t('explorer.drawer.loading')} className="flex flex-col gap-2 px-4 py-4">
        <Skeleton className="h-3 w-2/3" />
        <Skeleton className="h-3 w-1/2" />
        <Skeleton className="h-3 w-3/5" />
      </div>
    );
  }

  return (
    <div className="flex items-center justify-between gap-3 px-4 py-4 font-sans text-xs text-tx-2">
      <span>{t('waterfall.failed')}</span>
      <button
        type="button"
        onClick={() => void read.refetch()}
        className="inline-flex h-7 items-center gap-1.5 rounded-md border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
      >
        <RefreshCw aria-hidden className="h-3.5 w-3.5" />
        {t('overview.retry')}
      </button>
    </div>
  );
}

/** How the load of the view went, said in a sentence: how long it took, the moments that matter, and how much it asked for. */
function Sentence({ view, resources, tasks }: { view: ViewFacts; resources: number; tasks: number }) {
  const { t } = useTranslation('rum');
  const loaded = view.loading ?? markOf(view, 'load');
  const firstByte = markOf(view, 'first_byte');
  const lcp = markOf(view, 'lcp');
  const grade = lcp === null ? null : gradeOf(LCP_LEVEL, lcp);

  const clauses: React.ReactNode[] = [];

  if (loaded !== null) {
    clauses.push(<Trans key="loaded" i18nKey="waterfall.sentence.loaded" ns="rum" values={{ value: formatDurationMs(loaded) }} components={{ n: FIGURE }} />);
  }

  if (firstByte !== null) {
    clauses.push(<Trans key="first_byte" i18nKey="waterfall.sentence.first_byte" ns="rum" values={{ value: formatDurationMs(firstByte) }} components={{ n: FIGURE }} />);
  }

  if (lcp !== null && grade !== null) {
    clauses.push(
      <Trans
        key="lcp"
        i18nKey={view.lcpElement ? 'waterfall.sentence.lcp_element' : 'waterfall.sentence.lcp'}
        ns="rum"
        values={{ value: formatDurationMs(lcp), grade: t(`overview.vitals.grade.${grade}`), element: view.lcpElement }}
        components={{
          n: FIGURE,
          grade: <span className={cn('font-strong', GRADE_TEXT[grade])} />,
          code: <code className="font-mono text-tx-0" />,
        }}
      />,
    );
  }

  const counts = (
    <Trans
      i18nKey="waterfall.sentence.during"
      ns="rum"
      components={{
        resources: <Trans i18nKey="waterfall.sentence.resources" ns="rum" count={resources} values={{ count: resources }} components={{ n: FIGURE }} />,
        tasks: <Trans i18nKey="waterfall.sentence.long_tasks" ns="rum" count={tasks} values={{ count: tasks }} components={{ n: FIGURE }} />,
      }}
    />
  );

  return (
    <p data-rum-waterfall-sentence="" className="m-0 font-sans type-section-title font-strong text-tx-1">
      {clauses.map((clause, index) => (
        <React.Fragment key={index}>
          {index > 0 && t('waterfall.sentence.join')}
          {clause}
        </React.Fragment>
      ))}
      {clauses.length > 0 && t('waterfall.sentence.end')}
      {counts}
    </p>
  );
}

/** The numbers over the rows, the lines where the load went through a moment, and the names of both. */
function Scale({ view, axis }: { view: ViewFacts; axis: Axis }) {
  const { t } = useTranslation('rum');
  const labels = view.marks.map((mark) => `${t(`waterfall.marks.${mark.key}`)} ${formatDurationMs(mark.at)}`);
  const [track, width] = useMeasuredWidth<HTMLDivElement>();
  const lines = placeLabels(
    view.marks.map((mark, index) => ({ share: shareOf(mark.at, axis), chars: (labels[index] as string).length })),
    MARK_TOP.length,
    width > 0 ? width : undefined,
  );

  return (
    <div aria-hidden className={cn('pointer-events-none absolute inset-0', COLUMNS)}>
      <span />
      <div ref={track} className="relative">
        {axis.ticks.map((tick, index) => (
          <React.Fragment key={tick}>
            <span className="absolute inset-y-0 border-l border-[var(--row-rule)]" style={{ left: `${shareOf(tick, axis) * 100}%` }} />
            <span
              className={cn('absolute top-1.5 whitespace-nowrap font-sans text-xs text-tx-3', index === axis.ticks.length - 1 && '-translate-x-full pr-1')}
              style={{ left: `${shareOf(tick, axis) * 100}%` }}
            >
              {tickLabel(tick)}
            </span>
          </React.Fragment>
        ))}
        {view.marks.map((mark, index) => {
          const share = shareOf(mark.at, axis);
          const top = MARK_TOP[lines[index] ?? 0] as number;
          const grade = mark.key === 'lcp' ? gradeOf(LCP_LEVEL, mark.at) : null;
          const label = labels[index] as string;

          return (
            <React.Fragment key={mark.key}>
              <span className="absolute bottom-0 border-l border-dashed" style={{ left: `${share * 100}%`, top, borderColor: MARK_LINE[mark.key] }} />
              <span
                data-rum-mark={mark.key}
                title={label}
                className={cn(
                  'absolute whitespace-nowrap bg-[var(--functional-surface)] px-[3px] font-sans text-xs font-strong',
                  grade ? GRADE_TEXT[grade] : MARK_INK[mark.key as Exclude<MarkKey, 'lcp'>],
                  share < 0.1 ? '' : share > 0.88 ? '-translate-x-full' : '-translate-x-1/2',
                )}
                style={{ left: `${share * 100}%`, top }}
              >
                {label}
              </span>
            </React.Fragment>
          );
        })}
      </div>
    </div>
  );
}

const WAIT = 'var(--chart-8)';
const DOWNLOAD = 'var(--chart-7)';

/** One request: what it was and where, and a bar from when it began to when it ended, cut where the first byte came. */
function ResourceRow({
  resource,
  axis,
  pageHost,
  selected,
  onPick,
}: {
  resource: WaterfallResource;
  axis: Axis;
  pageHost: string | null;
  selected: boolean;
  onPick: () => void;
}) {
  const { t } = useTranslation('rum');
  const left = shareOf(resource.start, axis);
  const width = Math.max(0.004, shareOf(resource.start + resource.duration, axis) - left);
  const late = left + width > 0.82;
  const name = [resource.method && resource.method !== 'GET' ? resource.method : null, resource.path].filter(Boolean).join(' ');

  return (
    <button
      type="button"
      aria-pressed={selected}
      data-rum-waterfall-row={resource.id}
      data-rum-resource-type={resource.type}
      onClick={onPick}
      className={cn(
        COLUMNS,
        'relative h-8 w-full items-center border-0 border-t border-[var(--row-rule)] bg-transparent py-0 text-left font-sans text-xs outline-none hover:bg-bg-2 focus-visible:bg-bg-3',
        selected && 'bg-[var(--floating-item-selected)]',
      )}
    >
      <span className="flex min-w-0 items-baseline gap-1.5 whitespace-nowrap">
        <span className={cn('w-[54px] shrink-0 font-strong', typeInk(resource.type))}>{resource.type}</span>
        {resource.host && resource.host !== pageHost && <span className="shrink-[3] truncate text-tx-3">{resource.host}</span>}
        <span className="min-w-0 truncate text-tx-0" title={resource.url}>
          {name}
        </span>
        {resource.blocking && <span className="shrink-0 text-[var(--orange-ink)]">{t('waterfall.row.blocking')}</span>}
        {resource.lcp && <span className="shrink-0 font-strong text-[var(--yellow-ink)]">{t('waterfall.row.lcp')}</span>}
        {resource.failed && <span className={cn('shrink-0 font-mono', statusInk(resource.status))}>{resource.status}</span>}
      </span>
      <span className="relative h-8">
        <span
          aria-hidden
          className="absolute top-[11px] flex h-[10px]"
          style={{ left: `${left * 100}%`, width: `${width * 100}%` }}
        >
          {resource.wait !== null && resource.download !== null ? (
            <>
              <span style={{ flex: `${Math.max(resource.wait, 0.001)} 1 0`, background: WAIT, opacity: 0.9 }} />
              <span style={{ flex: `${Math.max(resource.download, 0.001)} 1 0`, background: DOWNLOAD, opacity: 0.9 }} />
            </>
          ) : (
            <span className="flex-1 bg-tx-4" />
          )}
        </span>
        <span
          className={cn('absolute top-[7px] whitespace-nowrap text-xs tabular-nums', resource.slow ? 'text-[var(--orange-ink)]' : 'text-tx-3', resource.failed && 'text-[var(--red-ink)]')}
          style={late ? { right: `calc(${(1 - left) * 100}% + 6px)` } : { left: `calc(${(left + width) * 100}% + 6px)` }}
        >
          {formatDurationMs(resource.duration)}
        </span>
      </span>
    </button>
  );
}

/** The main thread: the long tasks of the view on the same axis as the requests. */
function Lane({
  tasks,
  total,
  axis,
  selected,
  onPick,
}: {
  tasks: readonly WaterfallTask[];
  total: number;
  axis: Axis;
  selected: string | null;
  onPick: (id: string) => void;
}) {
  const { t } = useTranslation('rum');

  return (
    <div data-rum-waterfall-lane="" className={cn(COLUMNS, 'h-9 items-center border-t border-[var(--row-rule)] bg-[color-mix(in_srgb,var(--red)_6%,transparent)] font-sans text-xs')}>
      <span className="flex items-baseline gap-1.5 whitespace-nowrap">
        <span className="font-strong text-[var(--red-ink)]">{t('waterfall.lane.title')}</span>
        <span className="text-tx-3">{t('waterfall.lane.count', { count: total })}</span>
      </span>
      <span className="relative h-9">
        {tasks
          .filter((task) => task.start < axis.max)
          .map((task) => {
            const left = shareOf(task.start, axis);
            const width = Math.max(0.004, shareOf(task.start + task.duration, axis) - left);

            return (
              <button
                key={task.id}
                type="button"
                aria-pressed={selected === task.id}
                aria-label={t('waterfall.lane.task', { duration: formatDurationMs(task.duration) })}
                data-rum-waterfall-task={task.id}
                onClick={() => onPick(task.id)}
                className={cn(
                  'absolute top-[10px] h-[14px] min-w-[3px] cursor-pointer rounded-[2px] border-0 bg-[var(--red)] p-0 opacity-85 outline-none hover:opacity-100 focus-visible:opacity-100',
                  selected === task.id && 'opacity-100 ring-2 ring-tx-0',
                )}
                style={{ left: `${left * 100}%`, width: `${width * 100}%` }}
              />
            );
          })}
      </span>
    </div>
  );
}

/**
 * How a view loaded: the requests it made on a time axis from its start, each cut where the first
 * byte came, the moments the load went through (first byte, first and largest paint, the document
 * ready, loaded) as lines over them, and the long tasks of the main thread over the same time. A
 * request opens at its right, and so does a long task; the file the largest paint waited for says so.
 * The view's own columns are the ones to read; the requests and the long tasks are read for its id.
 */
export function ViewWaterfall({ row, focus, focusAt }: { row: RumRow; focus?: string | undefined; focusAt?: number | undefined }) {
  const { t } = useTranslation('rum');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const view = React.useMemo(() => viewFacts(row), [row]);
  const { resources: resourceRead, tasks: taskRead } = useViewReads(view);

  const resources = React.useMemo(() => readResources(resourceRead.data?.items ?? [], view), [resourceRead.data, view]);
  const tasks = React.useMemo(() => readTasks(taskRead.data?.items ?? [], view), [taskRead.data, view]);
  const axis = React.useMemo(() => axisFor(view, resources), [resources, view]);

  const [type, setType] = React.useState('');
  const [show, setShow] = React.useState<ShowFilter>('all');
  const [picked, setPicked] = React.useState<string | null>(focus ?? null);
  const [expanded, setExpanded] = React.useState(false);

  const found = React.useMemo(() => drawn(resources, axis, { type, show }), [axis, resources, show, type]);
  const pickedIndex = found.rows.findIndex((entry) => entry.id === picked);
  const open = expanded || pickedIndex >= FIRST_ROWS;
  const shown = open ? found.rows : found.rows.slice(0, FIRST_ROWS);
  const hidden = open ? [] : found.rows.slice(FIRST_ROWS);

  // What was opened from another page may have happened after the load: it is read by itself, and its panel says so.
  const drawnFocus = focus === undefined || resources.some((entry) => entry.id === focus) || tasks.some((entry) => entry.id === focus);
  const lateFocus = useQuery({
    queryKey: ['rum', 'waterfall', orgId, 'focus', view.id, focus, focusAt],
    queryFn: async () => {
      const [requests, blocked] = await Promise.all([
        rumApi.search(focusQuery('resources', focus as string, focusAt as number)),
        rumApi.search(focusQuery('long_tasks', focus as string, focusAt as number)),
      ]);

      return { resource: readResources(requests.items, view)[0] ?? null, task: readTasks(blocked.items, view)[0] ?? null };
    },
    enabled: Boolean(orgId && focus && focusAt && resourceRead.isSuccess && taskRead.isSuccess && !drawnFocus),
    staleTime: 60_000,
  });

  const resource = resources.find((entry) => entry.id === picked) ?? (picked === focus ? (lateFocus.data?.resource ?? undefined) : undefined);
  const task = resource ? undefined : (tasks.find((entry) => entry.id === picked) ?? (picked === focus ? (lateFocus.data?.task ?? undefined) : undefined));
  const afterLoad = resource !== undefined && !resources.some((entry) => entry.id === resource.id);
  const hiddenSpeed = mostCameInUnder(hidden);
  // What the view asked for after the load is not read: the count it carries says how many, where there are more than were found early.
  const earlyCount = resources.filter((entry) => entry.start < axis.max).length;
  const late = view.resources === null ? found.late : Math.max(found.late, view.resources - earlyCount);

  return (
    <div data-rum-waterfall="" className="space-y-3">
      <Sentence view={view} resources={view.resources ?? resources.length} tasks={view.longTasks ?? tasks.length} />
      <div className="flex flex-wrap items-start gap-3">
        <section className="min-w-0 flex-[1_1_520px] overflow-hidden rounded-md border border-bd-0">
          <div className="flex flex-wrap items-center gap-x-2.5 gap-y-2 px-4 py-2.5">
            <FittedSelect
              aria-label={t('waterfall.filters.type')}
              data-rum-waterfall-type=""
              value={type === '' ? 'all' : type}
              onValueChange={(next) => setType(next === 'all' ? '' : next)}
              options={[{ value: 'all', label: t('waterfall.filters.all') }, ...typesOf(resources).map((entry) => ({ value: entry, label: entry }))]}
              maxWidth={200}
              prefix={t('waterfall.filters.type')}
              className={cn(CONTROL, 'font-strong text-tx-1')}
            />
            <FittedSelect
              aria-label={t('waterfall.filters.show')}
              data-rum-waterfall-show=""
              value={show}
              onValueChange={(next) => setShow(next as ShowFilter)}
              options={SHOW_FILTERS.map((entry) => ({ value: entry, label: t(`waterfall.filters.shows.${entry}`) }))}
              maxWidth={240}
              prefix={t('waterfall.filters.show')}
              className={cn(CONTROL, 'font-strong text-tx-1')}
            />
            <span className="ml-auto inline-flex items-center gap-3 font-sans text-xs text-tx-1">
              {([
                ['wait', WAIT],
                ['download', DOWNLOAD],
                ['long_task', 'var(--red)'],
              ] as const).map(([key, fill]) => (
                <span key={key} className="inline-flex items-center gap-1.5">
                  <span aria-hidden className="h-1.5 w-2.5 rounded-[2px]" style={{ background: fill }} />
                  {t(`waterfall.legend.${key}`)}
                </span>
              ))}
            </span>
          </div>

          <div className="relative border-t border-[var(--row-rule)]">
            <Scale view={view} axis={axis} />
            <div style={{ height: HEAD_HEIGHT }} />
            {taskRead.isSuccess || (view.longTasks ?? 0) === 0 ? (
              <Lane tasks={tasks} total={view.longTasks ?? tasks.length} axis={axis} selected={picked} onPick={setPicked} />
            ) : null}
            {resourceRead.isSuccess ? (
              <>
                {shown.map((entry) => (
                  <ResourceRow key={entry.id} resource={entry} axis={axis} pageHost={view.host} selected={picked === entry.id} onPick={() => setPicked(entry.id)} />
                ))}
                {found.rows.length === 0 && (
                  <div className="flex h-12 items-center justify-center border-t border-[var(--row-rule)] font-sans text-xs text-tx-3">
                    {resources.length === 0 ? t('waterfall.none') : t('waterfall.no_match')}
                  </div>
                )}
              </>
            ) : (
              <ReadState read={resourceRead} />
            )}
            {hidden.length > 0 && (
              <div className="flex min-h-8 items-center justify-center gap-3 border-t border-[var(--row-rule)] font-sans text-xs text-tx-3">
                <span>{t('waterfall.more', { count: hidden.length, duration: formatDurationMs(hiddenSpeed ?? 0) })}</span>
                <button
                  type="button"
                  onClick={() => setExpanded(true)}
                  className="border-0 bg-transparent p-0 font-sans text-xs font-strong text-[var(--blue-ink)] outline-none hover:underline focus-visible:underline"
                >
                  {t('waterfall.expand')}
                </button>
              </div>
            )}
            {open && found.rows.length > FIRST_ROWS && (
              <div className="flex min-h-8 items-center justify-center border-t border-[var(--row-rule)] font-sans text-xs">
                <button
                  type="button"
                  onClick={() => setExpanded(false)}
                  className="border-0 bg-transparent p-0 font-sans text-xs font-strong text-[var(--blue-ink)] outline-none hover:underline focus-visible:underline"
                >
                  {t('waterfall.collapse')}
                </button>
              </div>
            )}
          </div>
          {resourceRead.data?.hasMore && (
            <p data-rum-waterfall-truncated="" className="m-0 border-t border-[var(--row-rule)] px-4 py-2 font-sans text-xs text-tx-3">
              {t('waterfall.truncated', { count: resources.length })}
            </p>
          )}
          {late > 0 && (
            <p data-rum-waterfall-late="" className="m-0 border-t border-[var(--row-rule)] px-4 py-2 font-sans text-xs text-tx-3">
              {t('waterfall.late', { count: late })}
            </p>
          )}
        </section>
        {(resource || task) && (
          <div className="min-w-0 flex-[0_0_340px] max-w-full">
            {resource ? (
              <ResourceDetail resource={resource} lcp={view.lcpParts} afterLoad={afterLoad} onClose={() => setPicked(null)} />
            ) : task ? (
              <TaskDetail task={task} onClose={() => setPicked(null)} />
            ) : null}
          </div>
        )}
      </div>
    </div>
  );
}
