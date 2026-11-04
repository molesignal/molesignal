import { useQuery } from '@tanstack/react-query';
import { ChevronDown, ChevronUp, Maximize2, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as rumApi from '@/api/rum';
import type { RumField, RumRow } from '@/api/rum';
import { useTimeFormatter } from '@/lib/time';
import { JsonView } from '@/shell/JsonEditor';
import { cn } from '@/shell/lib/cn';
import { ResizeHandle } from '@/shell/ResizeHandle';
import { SignalReference } from '@/shell/SignalReference';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Button } from '@/shell/ui/button';
import { Sheet, SheetClose, SheetContent, SheetTitle } from '@/shell/ui/sheet';
import { Skeleton } from '@/shell/ui/skeleton';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { describeCell } from './cells';
import { type RumTab, rowId, rowTitle } from './model';
import { ViewWaterfall } from '../waterfall/Waterfall';

/** The row open in the drawer. */
export interface RumDrawerState {
  tab: RumTab;
  row: RumRow;
  open: boolean;
  /** Where the row happened, for a page that knows it better than the row it holds does. */
  span?: { from: number; to: number } | undefined;
  /** The request or the long task the waterfall of a view opens on, and when it happened, in microseconds. */
  focus?: string | undefined;
  focusAt?: number | undefined;
}

/** The neighbours of the row in the list it was opened from. */
export interface DrawerNeighbours {
  onPrevious?: (() => void) | undefined;
  onNext?: (() => void) | undefined;
}

/** What each type says of itself first, before every field it has. */
const FACTS: Record<RumTab, readonly string[]> = {
  sessions: [
    'usr.id',
    'usr.email',
    'session.duration',
    'session.view.count',
    'session.action.count',
    'session.error.count',
    'session.frustration.count',
    'browser.name',
    'os.name',
    'device.type',
    'geo.country_iso_code',
    'application.id',
    'version',
    'session.has_replay',
  ],
  views: [
    'view.url',
    'view.loading_time',
    'view.time_spent',
    'view.performance.lcp.timestamp',
    'view.performance.fcp.timestamp',
    'view.performance.inp.duration',
    'view.performance.cls.score',
    'view.first_byte',
    'view.action.count',
    'view.error.count',
    'view.resource.count',
    'session.id',
  ],
  actions: [
    'action.type',
    'action.target.name',
    'action.target.selector',
    'action.loading_time',
    'action.frustration.type',
    'view.url_path',
    'session.id',
  ],
  errors: [
    'error.message',
    'error.type',
    'error.source',
    'error.handling',
    'error.is_crash',
    'error.issue_id',
    'view.url_path',
    'version',
    'session.id',
  ],
  resources: [
    'resource.method',
    'resource.url',
    'resource.status_code',
    'resource.duration',
    'resource.size',
    'resource.type',
    'resource.trace_id',
    'resource.span_id',
    'view.url_path',
    'session.id',
  ],
  long_tasks: [
    'long_task.duration',
    'long_task.blocking_duration',
    'long_task.entry_type',
    'long_task.start_time',
    'view.url_path',
    'session.id',
  ],
  logs: ['level', 'message', 'log.logger', 'log.origin', 'log.error_type', 'view.url_path', 'session.id'],
};

/** Fields the drawer shows in a place of their own, not among the facts or the rest. */
const SET_APART = new Set(['error.stack', 'error.frames']);

const MIN_WIDTH = 420;
/** The room the list keeps beside the drawer at the least. */
const LIST_ROOM = 48;
const DEFAULT_SHARE = 0.46;
/** A view opens wide: its waterfall is a chart with a panel beside it. */
const VIEW_SHARE = 0.8;

function useWindowWidth(): number {
  const [width, setWidth] = React.useState(() => (typeof window === 'undefined' ? 1440 : window.innerWidth));

  React.useEffect(() => {
    const measure = () => setWidth(window.innerWidth);
    window.addEventListener('resize', measure);
    return () => window.removeEventListener('resize', measure);
  }, []);

  return width;
}

/** A second either side of the row: a row is dated at the moment it happened, and read by its id. */
const LOOKUP_PADDING_MICROS = 1_000_000;

function isPresent(value: unknown): boolean {
  return value !== null && value !== undefined && value !== '';
}

/** The page of the session a row belongs to; none for an error of a session that was not kept, which is kept alone. */
export function sessionHref(row: RumRow): string | null {
  const id = row['session.id'];
  if (row['retention.filter'] === 'errors') return null;
  return typeof id === 'string' && id !== '' ? `/rum/sessions/view/${encodeURIComponent(id)}` : null;
}

/**
 * A row opened from the results without leaving them: its facts and every field it has, in a
 * drawer whose left edge the reader drags. The keys J and K, or the arrows in its head, step to
 * the next and the previous row of the list; closing it gives the focus back to what opened it.
 */
export function RumDetailDrawer({
  state,
  fields,
  onClose,
  neighbours,
  returnFocusRef,
}: {
  state: RumDrawerState | null;
  fields: readonly RumField[];
  onClose: () => void;
  neighbours: DrawerNeighbours;
  /** What opened the drawer. */
  returnFocusRef: React.RefObject<HTMLElement | null>;
}) {
  const { t } = useTranslation('rum');
  const windowWidth = useWindowWidth();
  const maxWidth = Math.max(MIN_WIDTH, windowWidth - LIST_ROOM);
  const [chosen, setChosen] = React.useState<number | null>(null);
  const share = state?.tab === 'views' ? VIEW_SHARE : DEFAULT_SHARE;
  const width = Math.min(maxWidth, chosen ?? Math.max(MIN_WIDTH, Math.round(windowWidth * share)));

  // Read on the way down: a control inside that keeps its keys to itself still lets J and K by.
  const onKeyDownCapture = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (!event.currentTarget.contains(event.target as Node)) return;
    if ((event.target as HTMLElement).closest('input, textarea, [contenteditable="true"], [role="menu"]')) return;

    if (event.key === 'j' && neighbours.onNext) {
      event.preventDefault();
      neighbours.onNext();
    } else if (event.key === 'k' && neighbours.onPrevious) {
      event.preventDefault();
      neighbours.onPrevious();
    }
  };

  // A row opened by a page that holds no more than its id is named once it has been read whole.
  const whole = useQuery({ ...entityOptions(state?.tab ?? 'views', state?.row ?? NO_ROW, state?.span), enabled: Boolean(state?.open) });
  const shown = whole.data;
  const href = state ? sessionHref(shown ?? state.row) : null;

  return (
    <Sheet open={Boolean(state?.open)} onOpenChange={(open) => !open && onClose()}>
      <SheetContent
        side="right"
        hideClose
        // Under the top bar, and light enough to keep the list in sight.
        overlayClassName="top-topbar bg-overlay-soft"
        aria-describedby={undefined}
        data-rum-drawer=""
        onKeyDownCapture={onKeyDownCapture}
        onCloseAutoFocus={(event) => {
          const opener = returnFocusRef.current;
          if (!opener?.isConnected) return;
          event.preventDefault();
          opener.focus();
        }}
        className={cn('bottom-0 top-topbar flex h-auto max-w-none flex-row gap-0 border-0 bg-[var(--functional-surface)] p-0 shadow-drawer sm:max-w-none')}
        style={{ width }}
      >
        <div className="absolute inset-y-0 left-0 z-30 -translate-x-1/2">
          <ResizeHandle
            label={t('explorer.drawer.resize')}
            width={width}
            min={MIN_WIDTH}
            max={maxWidth}
            grows="left"
            defaultWidth={Math.round(windowWidth * share)}
            onWidthChange={setChosen}
            quiet
            className="h-full"
          />
        </div>
        <SheetTitle className="sr-only">{t('explorer.drawer.title')}</SheetTitle>
        {state && (
          <div className="flex min-h-0 min-w-0 flex-1 flex-col">
            <div className="flex min-h-12 shrink-0 items-center gap-2 border-b border-bd-0 px-4 py-2">
              <div className="min-w-0 flex-1">
                <p className="m-0 truncate font-sans text-sm font-strong text-tx-0" title={rowTitle(state.tab, shown ?? state.row)}>
                  {rowTitle(state.tab, shown ?? state.row) || t(`explorer.types.${state.tab}`)}
                </p>
                <p className="m-0 truncate font-sans text-xs text-tx-3">
                  {t(`explorer.types.${state.tab}`)} · {rowId(state.tab, state.row)}
                </p>
              </div>
              <HeadButton label={t('explorer.drawer.previous')} onClick={neighbours.onPrevious} disabled={!neighbours.onPrevious}>
                <ChevronUp className="h-4 w-4" />
              </HeadButton>
              <HeadButton label={t('explorer.drawer.next')} onClick={neighbours.onNext} disabled={!neighbours.onNext}>
                <ChevronDown className="h-4 w-4" />
              </HeadButton>
              {href && (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button variant="ghost" size="icon" className="h-8 w-8" asChild>
                      <Link to={href} aria-label={t('explorer.drawer.open_session')} data-testid="rum-drawer-session">
                        <Maximize2 className="h-4 w-4" />
                      </Link>
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>{t('explorer.drawer.open_session')}</TooltipContent>
                </Tooltip>
              )}
              <SheetClose asChild>
                <Button variant="ghost" size="icon" className="h-8 w-8" aria-label={t('explorer.drawer.close')}>
                  <X className="h-4 w-4" />
                </Button>
              </SheetClose>
            </div>
            <RumRowBody
              key={`${state.tab}:${rowId(state.tab, state.row)}`}
              tab={state.tab}
              listed={state.row}
              fields={fields}
              span={state.span}
              focus={state.focus}
              focusAt={state.focusAt}
            />
          </div>
        )}
      </SheetContent>
    </Sheet>
  );
}

function HeadButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick?: (() => void) | undefined;
  disabled?: boolean | undefined;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="ghost" size="icon" className="h-8 w-8" aria-label={label} onClick={onClick} disabled={disabled}>
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

const NO_ROW: RumRow = { timestamp: 0 };

/** How a row is read whole: by its id, over where it happened. The head of the drawer and its body read it alike, and so once. */
function entityOptions(tab: RumTab, listed: RumRow, span: { from: number; to: number } | undefined) {
  const id = rowId(tab, listed);
  const lookup = span ?? {
    from: listed.timestamp - LOOKUP_PADDING_MICROS,
    to: listed.timestamp + LOOKUP_PADDING_MICROS,
  };

  return {
    queryKey: ['rum', 'entity', tab, id, lookup.from, lookup.to],
    queryFn: ({ signal }: { signal: AbortSignal }) => rumApi.entity(tab, id, lookup, signal),
    enabled: id !== '',
    staleTime: 60_000,
    retry: false,
  };
}

/** What the drawer says of a row: it is listed with the columns of the table, and read whole in the background. */
function RumRowBody({
  tab,
  listed,
  fields,
  span,
  focus,
  focusAt,
}: {
  tab: RumTab;
  listed: RumRow;
  fields: readonly RumField[];
  span?: { from: number; to: number } | undefined;
  focus?: string | undefined;
  focusAt?: number | undefined;
}) {
  const { t, i18n } = useTranslation('rum');
  const fmt = useTimeFormatter();
  const language = i18n.resolvedLanguage ?? i18n.language;
  const whole = useQuery(entityOptions(tab, listed, span));

  const row = whole.data ?? listed;
  // The loading of a page is told for a view of a browser; a screen of an app has no such story. A row
  // that does not say where it was sent from yet (the table lists no such column) is told by its page.
  const loading = tab === 'views' && (row.source === undefined ? isPresent(row['view.url_path']) : row.source === 'browser');
  const [section, setSection] = React.useState<'waterfall' | 'fields'>('waterfall');
  const byName = React.useMemo(() => new Map(fields.map((field) => [field.name, field])), [fields]);
  const present = React.useMemo(
    () => Object.keys(row).filter((name) => name !== 'timestamp' && !SET_APART.has(name) && isPresent(row[name])),
    [row],
  );
  const facts = FACTS[tab].filter((name) => isPresent(row[name]));
  const rest = present.filter((name) => !facts.includes(name)).sort();
  const groups = React.useMemo(() => groupByHead(rest), [rest]);
  const stack = typeof row['error.stack'] === 'string' ? (row['error.stack'] as string) : null;

  const show = (name: string): React.ReactNode => {
    const field = byName.get(name);
    const value = row[name];

    // The issue the error belongs to opens on the errors page, in its drawer.
    if (name === 'error.issue_id' && typeof value === 'string') {
      const application = row['application.id'];
      const params = new URLSearchParams({ issue: value, ...(typeof application === 'string' && { app: application }) });

      return (
        <Link to={`/rum/errors?${params}`} className="text-[var(--blue-ink)] outline-none hover:underline focus-visible:underline">
          {value}
        </Link>
      );
    }

    if (name === 'resource.trace_id' && typeof value === 'string') {
      return (
        <SignalReference type="trace_id" value={value}>
          {value}
        </SignalReference>
      );
    }

    if (!field) return typeof value === 'string' ? value : JSON.stringify(value);

    if (field.dataType === 'json' && value !== null && typeof value === 'object') {
      return <JsonView value={value} ariaLabel={name} maxHeight={320} />;
    }

    return describeCell(field, value, language)?.text ?? '—';
  };

  const waterfall = loading && section === 'waterfall';

  return (
    <div data-rum-drawer-body className="min-h-0 flex-1 overflow-auto px-4 py-3">
      <p className="m-0 mb-3 font-sans text-xs text-tx-3">{fmt.micros(row.timestamp)}</p>
      {loading && (
        <ViewTabs
          label={t('explorer.drawer.sections.aria')}
          value={section}
          onValueChange={setSection}
          className="mb-3"
          dataAttributes={{ 'data-rum-drawer-sections': '' }}
          items={[
            { key: 'waterfall', label: t('explorer.drawer.sections.waterfall') },
            { key: 'fields', label: t('explorer.drawer.sections.fields') },
          ]}
        />
      )}
      {whole.isError && (
        <div data-rum-drawer-failed="" className="mb-3 flex items-center justify-between gap-3 font-sans text-xs text-tx-2">
          <span>{t('explorer.drawer.failed')}</span>
          <Button variant="ghost" size="sm" onClick={() => void whole.refetch()}>
            {t('overview.retry')}
          </Button>
        </div>
      )}
      {(whole.isLoading || (waterfall && !whole.data)) && (
        <div role="status" aria-label={t('explorer.drawer.loading')} className="mb-3 flex flex-col gap-[6px]">
          <Skeleton className="h-[14px] w-2/3" />
        </div>
      )}
      {waterfall ? (
        whole.data && <ViewWaterfall row={whole.data} focus={focus} focusAt={focusAt} />
      ) : (
        <>
          {facts.length > 0 && <FactList names={facts} show={show} />}
          {stack && (
            <section className="mt-4">
              <h3 className="m-0 mb-1.5 font-sans text-xs font-strong text-tx-2">{t('explorer.drawer.stack')}</h3>
              <pre
                data-rum-stack
                className="m-0 max-h-72 overflow-auto rounded-md bg-[var(--control-surface)] p-3 font-mono text-xs leading-relaxed text-tx-1"
              >
                {stack}
              </pre>
            </section>
          )}
          {groups.map(([head, names]) => (
            <section key={head} className="mt-4" data-rum-field-group={head}>
              <h3 className="m-0 mb-1.5 font-sans text-xs font-strong text-tx-2">
                {head || t('explorer.drawer.general')}
              </h3>
              <FactList names={names} show={show} />
            </section>
          ))}
        </>
      )}
    </div>
  );
}

function FactList({ names, show }: { names: readonly string[]; show: (name: string) => React.ReactNode }) {
  return (
    <dl className="m-0 grid grid-cols-[minmax(120px,0.8fr)_minmax(0,2fr)] gap-x-3 gap-y-2 font-sans text-xs">
      {names.map((name) => (
        <React.Fragment key={name}>
          <dt className="min-w-0 truncate text-tx-3" title={name}>
            {name}
          </dt>
          <dd className="m-0 min-w-0 break-words font-strong text-tx-1">{show(name)}</dd>
        </React.Fragment>
      ))}
    </dl>
  );
}

/** The names under the first part they share, in the order their first part first appears. */
function groupByHead(names: readonly string[]): Array<[string, string[]]> {
  const groups = new Map<string, string[]>();

  for (const name of names) {
    const dot = name.indexOf('.');
    const head = dot > 0 ? name.slice(0, dot) : '';
    groups.set(head, [...(groups.get(head) ?? []), name]);
  }

  return [...groups.entries()].sort(([left], [right]) => left.localeCompare(right));
}
