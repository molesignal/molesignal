import type { RumRow, TimelineItem } from '@/api/rum';

import type { ReplayMark } from '../../replay/ReplayPlayer';
import { SLOW_MS } from '../../waterfall/model';

// ---------------------------------------------------------------------------------------------
// What the rows of the tools are read out of: the items of the session's timeline.

const text = (row: RumRow, field: string): string | null => {
  const value = row[field];

  return typeof value === 'string' && value.trim() !== '' ? value : null;
};

const numeric = (row: RumRow, field: string): number | null => {
  const value = row[field];

  return typeof value === 'number' && Number.isFinite(value) ? value : null;
};

const NS_PER_MS = 1_000_000;

/** A request the page made, as the network tool lists it. */
export interface NetworkRow {
  key: string;
  /** When it began, in microseconds since the epoch. */
  at: number;
  view: string | null;
  method: string | null;
  host: string | null;
  /** What follows the host: the path with its query, or the whole address where it does not parse. */
  path: string;
  url: string;
  status: number | null;
  type: string;
  /** In milliseconds. */
  duration: number | null;
  size: number | null;
  /** It ended in an error, or in none of the answers a server gives. */
  failed: boolean;
}

/** The host and the rest of an address; an address that is not one is only a path. */
export function splitUrl(url: string): { host: string | null; path: string } {
  try {
    const parsed = new URL(url);

    return { host: parsed.host || null, path: `${parsed.pathname}${parsed.search}` || '/' };
  } catch {
    return { host: null, path: url };
  }
}

export function networkRows(items: readonly TimelineItem[]): NetworkRow[] {
  return items.flatMap((item, index): NetworkRow[] => {
    if (item.type !== 'resource') return [];

    const url = text(item, 'resource.url') ?? '';
    const parts = splitUrl(url);
    const status = numeric(item, 'resource.status_code');
    const duration = numeric(item, 'resource.duration');

    return [
      {
        key: text(item, 'resource.id') ?? `resource-${item.timestamp}-${index}`,
        at: item.timestamp,
        view: text(item, 'view.id'),
        method: text(item, 'resource.method'),
        host: text(item, 'resource.url_host') ?? parts.host,
        path: parts.path,
        url,
        status,
        type: text(item, 'resource.type') ?? 'other',
        duration: duration === null ? null : duration / NS_PER_MS,
        size: numeric(item, 'resource.size'),
        failed: status !== null && (status >= 400 || status === 0),
      },
    ];
  });
}

/** What went wrong, as the errors tool lists it. */
export interface ErrorRow {
  key: string;
  at: number;
  view: string | null;
  type: string | null;
  message: string;
  source: string | null;
  handling: string | null;
  /** The issue the error was grouped into. */
  issue: string | null;
  crash: boolean;
}

export function errorRows(items: readonly TimelineItem[]): ErrorRow[] {
  return items.flatMap((item, index): ErrorRow[] => {
    if (item.type !== 'error') return [];

    return [
      {
        key: text(item, 'error.id') ?? `error-${item.timestamp}-${index}`,
        at: item.timestamp,
        view: text(item, 'view.id'),
        type: text(item, 'error.type'),
        message: text(item, 'error.message') ?? text(item, 'error.type') ?? '',
        source: text(item, 'error.source'),
        handling: text(item, 'error.handling'),
        issue: text(item, 'error.issue_id'),
        crash: item['error.is_crash'] === true,
      },
    ];
  });
}

/** One page view of the session, as the performance tool lists it. */
export interface ViewRow {
  id: string;
  /** When it began, in microseconds since the epoch. */
  at: number;
  name: string;
  path: string | null;
  /** In milliseconds. */
  lcp: number | null;
  inp: number | null;
  cls: number | null;
  loading: number | null;
  resources: number;
  longTasks: number;
  errors: number;
  frustrations: number;
}

const ms = (value: number | null): number | null => (value === null ? null : value / NS_PER_MS);

export function viewRows(items: readonly TimelineItem[]): ViewRow[] {
  return items.flatMap((item): ViewRow[] => {
    const id = text(item, 'view.id');

    if (item.type !== 'view' || id === null) return [];

    const path = text(item, 'view.url_path');

    return [
      {
        id,
        at: item.timestamp,
        name: text(item, 'view.name') ?? path ?? id,
        path,
        lcp: ms(numeric(item, 'view.performance.lcp.timestamp')),
        inp: ms(numeric(item, 'view.performance.inp.duration')),
        cls: numeric(item, 'view.performance.cls.score'),
        loading: ms(numeric(item, 'view.loading_time')),
        resources: numeric(item, 'view.resource.count') ?? 0,
        longTasks: numeric(item, 'view.long_task.count') ?? 0,
        errors: numeric(item, 'view.error.count') ?? 0,
        frustrations: numeric(item, 'view.frustration.count') ?? 0,
      },
    ];
  });
}

/** What the page or the application wrote, as the logs tool lists it. */
export interface LogRow {
  key: string;
  at: number;
  level: string;
  message: string;
  logger: string | null;
  stack: string | null;
}

export function logRows(items: readonly TimelineItem[]): LogRow[] {
  return items.flatMap((item, index): LogRow[] => {
    if (item.type !== 'log') return [];

    return [
      {
        key: text(item, 'log.id') ?? `log-${item.timestamp}-${index}`,
        at: item.timestamp,
        level: text(item, 'level') ?? 'info',
        message: text(item, 'message') ?? '',
        logger: text(item, 'log.logger'),
        stack: text(item, 'log.stack'),
      },
    ];
  });
}

/** The tools' rows, each in the order things happened. */
export interface ToolRows {
  views: ViewRow[];
  network: NetworkRow[];
  errors: ErrorRow[];
  logs: LogRow[];
}

const byTime = <T extends { at: number }>(rows: T[]): T[] => rows.sort((a, b) => a.at - b.at);

export function toolRows(items: readonly TimelineItem[]): ToolRows {
  return {
    views: byTime(viewRows(items)),
    network: byTime(networkRows(items)),
    errors: byTime(errorRows(items)),
    logs: byTime(logRows(items)),
  };
}

// ---------------------------------------------------------------------------------------------
// Which of the rows are in view, and which one the replay is at.

/** The tabs of the tools. */
export type ToolTab = 'errors' | 'network' | 'performance' | 'logs' | 'comments';

export const TOOL_TABS: readonly ToolTab[] = ['errors', 'network', 'performance', 'logs', 'comments'];

/** What the tools open on: the errors where the session ran into any, else the requests. */
export const defaultTab = (rows: ToolRows): ToolTab => (rows.errors.length > 0 ? 'errors' : 'network');

export const parseTab = (value: string | null): ToolTab | null => TOOL_TABS.find((tab) => tab === value) ?? null;

export type Display = 'list' | 'waterfall';

export const parseDisplay = (value: string | null): Display => (value === 'waterfall' ? 'waterfall' : 'list');

/** Which of the requests of a list are shown: all of them, the slow ones, or the ones that failed. */
export type Show = 'all' | 'slow' | 'failed';

export const SHOWS: readonly Show[] = ['all', 'slow', 'failed'];

export const parseShow = (value: string | null): Show => SHOWS.find((show) => show === value) ?? 'all';

/** The requests a choice of what to show lets through, as the waterfall counts them: slow from half a second. */
export function shown(rows: readonly NetworkRow[], show: Show): NetworkRow[] {
  if (show === 'failed') return rows.filter((row) => row.failed);
  if (show === 'slow') return rows.filter((row) => (row.duration ?? 0) >= SLOW_MS);

  return [...rows];
}

/** The rows of one view, or all of them when no view is named. */
export const inView = <T extends { view: string | null }>(rows: readonly T[], view: string | null): T[] =>
  view === null ? [...rows] : rows.filter((row) => row.view === view);

/**
 * The view the replay is in: the last one that had begun by that moment, or the first before any had.
 * None when the session has no view row.
 */
export function viewAt(views: readonly ViewRow[], micros: number | null): ViewRow | null {
  const first = views[0];

  if (!first) return null;
  if (micros === null) return first;

  let found = first;

  for (const view of views) {
    if (view.at <= micros) found = view;
    else break;
  }

  return found;
}

/** The index of the last row that had happened by a moment, or -1 when none had; the rows are in order of time. */
export function lastBefore(rows: readonly { at: number }[], micros: number): number {
  let low = 0;
  let high = rows.length - 1;
  let found = -1;

  while (low <= high) {
    const middle = (low + high) >> 1;

    if ((rows[middle] as { at: number }).at <= micros) {
      found = middle;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }

  return found;
}

/** The time axis of the requests of a view: from the start of the view to the end of the last request, in microseconds. */
export interface Axis {
  start: number;
  /** How long it runs; never less than a tenth of a second, so that a view with one quick request has a bar to draw. */
  length: number;
}

const MIN_AXIS_MICROS = 100_000;

export function axisOf(rows: readonly NetworkRow[], start: number): Axis {
  const end = rows.reduce((latest, row) => Math.max(latest, row.at + (row.duration ?? 0) * 1_000), start);

  return { start, length: Math.max(MIN_AXIS_MICROS, end - start) };
}

/** Where a request's bar lies on the axis, as shares of it from 0 to 1. */
export function barOf(row: Pick<NetworkRow, 'at' | 'duration'>, axis: Axis): { left: number; width: number } {
  const left = Math.min(1, Math.max(0, (row.at - axis.start) / axis.length));
  const width = Math.min(1 - left, Math.max(0, ((row.duration ?? 0) * 1_000) / axis.length));

  return { left, width };
}

// ---------------------------------------------------------------------------------------------
// The ticks on the replay's ruler.

/**
 * The moments of the session worth finding: its errors, the actions that showed a sign of frustration,
 * and the lines where one page gave way to the next.
 */
export function replayMarks(
  items: readonly TimelineItem[],
  comments: readonly { atTime: number }[] = [],
  chapters: readonly { atTime: number }[] = [],
): ReplayMark[] {
  let firstView = true;

  const noted = [
    ...comments.map((comment): ReplayMark => ({ at: comment.atTime / 1_000, kind: 'comment' })),
    ...chapters.map((chapter): ReplayMark => ({ at: chapter.atTime / 1_000, kind: 'chapter' })),
  ];

  const happened = items.flatMap((item): ReplayMark[] => {
    const at = item.timestamp / 1_000;

    if (item.type === 'error') return [{ at, kind: 'error' }];
    if (item.type === 'action' && (numeric(item, 'action.frustration.count') ?? 0) > 0) return [{ at, kind: 'frustration' }];

    if (item.type === 'view') {
      const first = firstView;

      firstView = false;

      return first ? [] : [{ at, kind: 'view' }];
    }

    return [];
  });

  return [...happened, ...noted];
}
