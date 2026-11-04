import type { RumRow, RumSearchParams } from '@/api/rum';

// ---------------------------------------------------------------------------------------------
// What a view says about its own loading.

const NS = 1_000_000;

/** A time the SDK sends in nanoseconds, in milliseconds; none where the row has none. */
const millis = (row: RumRow, field: string): number | null => {
  const value = row[field];

  return typeof value === 'number' && Number.isFinite(value) ? value / NS : null;
};

const text = (row: RumRow, field: string): string | null => {
  const value = row[field];

  return typeof value === 'string' && value.trim() !== '' ? value : null;
};

const whole = (row: RumRow, field: string): number | null => {
  const value = row[field];

  return typeof value === 'number' && Number.isFinite(value) ? value : null;
};

/** The moments a page load is told by, in the order they usually come. */
export type MarkKey = 'first_byte' | 'fcp' | 'dcl' | 'lcp' | 'load';

const MARK_FIELDS: ReadonlyArray<readonly [MarkKey, string]> = [
  ['first_byte', 'view.first_byte'],
  ['fcp', 'view.performance.fcp.timestamp'],
  ['dcl', 'view.dom_content_loaded'],
  ['lcp', 'view.performance.lcp.timestamp'],
  ['load', 'view.load_event'],
];

export interface Mark {
  key: MarkKey;
  /** Milliseconds from the start of the view. */
  at: number;
}

/** What the paint of the largest element waited for, in milliseconds. */
export interface LcpParts {
  loadDelay: number;
  loadTime: number;
  renderDelay: number;
}

/** The columns of a view the waterfall is drawn from. */
export const VIEW_COLUMNS = [
  'view.id',
  'view.url_host',
  'view.url_path',
  'view.loading_time',
  'view.first_byte',
  'view.performance.fcp.timestamp',
  'view.dom_content_loaded',
  'view.performance.lcp.timestamp',
  'view.performance.lcp.target_selector',
  'view.performance.lcp.resource_url',
  'view.performance.lcp.sub_parts.load_delay',
  'view.performance.lcp.sub_parts.load_time',
  'view.performance.lcp.sub_parts.render_delay',
  'view.load_event',
  'view.time_spent',
  'view.resource.count',
  'view.long_task.count',
] as const;

export interface ViewFacts {
  id: string;
  /** When the view began, in microseconds: the resources and the long tasks are placed from here. */
  start: number;
  host: string | null;
  path: string | null;
  /** The moments the view has, in the order of time. */
  marks: Mark[];
  /** Until the page was loaded, in milliseconds. */
  loading: number | null;
  /** The element the largest paint was of, and the file where it is one. */
  lcpElement: string | null;
  lcpResource: string | null;
  lcpParts: LcpParts | null;
  /** How many resources and long tasks the view says it had. */
  resources: number | null;
  longTasks: number | null;
}

export function viewFacts(row: RumRow): ViewFacts {
  const marks = MARK_FIELDS.flatMap(([key, field]) => {
    const at = millis(row, field);

    return at === null || at < 0 ? [] : [{ key, at }];
  }).sort((a, b) => a.at - b.at);

  const parts = {
    loadDelay: millis(row, 'view.performance.lcp.sub_parts.load_delay'),
    loadTime: millis(row, 'view.performance.lcp.sub_parts.load_time'),
    renderDelay: millis(row, 'view.performance.lcp.sub_parts.render_delay'),
  };

  return {
    id: text(row, 'view.id') ?? '',
    start: row.timestamp,
    host: text(row, 'view.url_host'),
    path: text(row, 'view.url_path'),
    marks,
    loading: millis(row, 'view.loading_time'),
    lcpElement: text(row, 'view.performance.lcp.target_selector'),
    lcpResource: text(row, 'view.performance.lcp.resource_url'),
    lcpParts:
      parts.loadDelay !== null && parts.loadTime !== null && parts.renderDelay !== null
        ? { loadDelay: parts.loadDelay, loadTime: parts.loadTime, renderDelay: parts.renderDelay }
        : null,
    resources: whole(row, 'view.resource.count'),
    longTasks: whole(row, 'view.long_task.count'),
  };
}

/** A moment of the view, if it has it. */
export const markOf = (view: ViewFacts, key: MarkKey): number | null => view.marks.find((mark) => mark.key === key)?.at ?? null;

// ---------------------------------------------------------------------------------------------
// The resources a view asked for.

export const SLOW_MS = 500;

/** The columns of a resource the waterfall is drawn from. */
export const RESOURCE_COLUMNS = [
  'resource.id',
  'resource.type',
  'resource.method',
  'resource.url',
  'resource.url_host',
  'resource.url_path',
  'resource.status_code',
  'resource.duration',
  'resource.size',
  'resource.transfer_size',
  'resource.render_blocking_status',
  'resource.protocol',
  'resource.delivery_type',
  'resource.redirect.duration',
  'resource.dns.duration',
  'resource.connect.duration',
  'resource.ssl.duration',
  'resource.first_byte.duration',
  'resource.download.duration',
  'resource.trace_id',
  'resource.span_id',
] as const;

/** The parts of a request, in the order they happen, in milliseconds. */
export interface Phases {
  redirect: number | null;
  dns: number | null;
  /** The connection without the TLS handshake inside it. */
  connect: number | null;
  tls: number | null;
  first_byte: number | null;
  download: number | null;
}

export interface WaterfallResource {
  id: string;
  type: string;
  method: string | null;
  url: string;
  host: string | null;
  path: string;
  status: number | null;
  /** Milliseconds from the start of the view. */
  start: number;
  duration: number;
  /** Until the first byte came, and what came after: both or neither. */
  wait: number | null;
  download: number | null;
  size: number | null;
  transfer: number | null;
  protocol: string | null;
  delivery: string | null;
  blocking: boolean;
  failed: boolean;
  slow: boolean;
  /** The file the largest paint was of. */
  lcp: boolean;
  traceId: string | null;
  phases: Phases;
}

/** A request that got no answer, or got one that says it failed. */
export const hasFailed = (status: number | null): boolean => status !== null && (status === 0 || status >= 400);

const withoutHash = (url: string): string => url.split('#')[0] ?? url;

function phasesOf(row: RumRow): Phases {
  const connect = millis(row, 'resource.connect.duration');
  const tls = millis(row, 'resource.ssl.duration');

  return {
    redirect: millis(row, 'resource.redirect.duration'),
    dns: millis(row, 'resource.dns.duration'),
    // The handshake is part of the connection in the timing the browser reports: told apart, not twice.
    connect: connect === null ? null : Math.max(0, connect - (tls ?? 0)),
    tls,
    first_byte: millis(row, 'resource.first_byte.duration'),
    download: millis(row, 'resource.download.duration'),
  };
}

export function readResources(rows: readonly RumRow[], view: ViewFacts): WaterfallResource[] {
  return rows
    .flatMap((row) => {
      const id = text(row, 'resource.id');
      const url = text(row, 'resource.url');
      const duration = millis(row, 'resource.duration');

      if (id === null || url === null || duration === null) return [];

      const status = whole(row, 'resource.status_code');
      const download = millis(row, 'resource.download.duration');
      const phases = phasesOf(row);
      const known = download !== null && download <= duration;

      return [
        {
          id,
          type: text(row, 'resource.type') ?? 'other',
          method: text(row, 'resource.method'),
          url,
          host: text(row, 'resource.url_host'),
          path: text(row, 'resource.url_path') ?? url,
          status,
          start: Math.max(0, (row.timestamp - view.start) / 1_000),
          duration,
          wait: known ? duration - download : null,
          download: known ? download : null,
          size: whole(row, 'resource.size'),
          transfer: whole(row, 'resource.transfer_size'),
          protocol: text(row, 'resource.protocol'),
          delivery: text(row, 'resource.delivery_type'),
          blocking: text(row, 'resource.render_blocking_status') === 'blocking',
          failed: hasFailed(status),
          slow: duration >= SLOW_MS,
          lcp: view.lcpResource !== null && withoutHash(view.lcpResource) === withoutHash(url),
          traceId: text(row, 'resource.trace_id'),
          phases,
        },
      ];
    })
    .sort((a, b) => a.start - b.start || a.id.localeCompare(b.id));
}

// ---------------------------------------------------------------------------------------------
// The long tasks that blocked the page.

/** The columns of a long task the waterfall is drawn from. */
export const TASK_COLUMNS = [
  'long_task.id',
  'long_task.duration',
  'long_task.blocking_duration',
  'long_task.entry_type',
  'long_task.top_script.source_url',
  'long_task.top_script.function',
  'long_task.top_script.invoker_type',
] as const;

export interface WaterfallTask {
  id: string;
  /** Milliseconds from the start of the view. */
  start: number;
  duration: number;
  /** The part of it that held up the user's input, where the browser says. */
  blocking: number | null;
  script: string | null;
  function: string | null;
  invoker: string | null;
}

export function readTasks(rows: readonly RumRow[], view: ViewFacts): WaterfallTask[] {
  return rows
    .flatMap((row) => {
      const id = text(row, 'long_task.id');
      const duration = millis(row, 'long_task.duration');

      if (id === null || duration === null) return [];

      return [
        {
          id,
          start: Math.max(0, (row.timestamp - view.start) / 1_000),
          duration,
          blocking: millis(row, 'long_task.blocking_duration'),
          script: text(row, 'long_task.top_script.source_url'),
          function: text(row, 'long_task.top_script.function'),
          invoker: text(row, 'long_task.top_script.invoker_type'),
        },
      ];
    })
    .sort((a, b) => a.start - b.start || a.id.localeCompare(b.id));
}

// ---------------------------------------------------------------------------------------------
// The time axis.

export interface Axis {
  /** Milliseconds from the start of the view to the end of the axis. */
  max: number;
  step: number;
  ticks: number[];
}

const STEPS = [50, 100, 200, 250, 500, 1_000, 2_000, 2_500, 5_000, 10_000, 15_000, 30_000, 60_000];

/** A frame for a load that ends at `end`: whole steps, at most five of them. */
export function axisOf(end: number): Axis {
  const target = Math.max(end * 1.05, 100);
  const step = STEPS.find((candidate) => target / candidate <= 5) ?? 120_000;
  const max = Math.ceil(target / step) * step;

  return { max, step, ticks: Array.from({ length: Math.round(max / step) + 1 }, (_, index) => index * step) };
}

/** Longest the axis is when the view does not say when it was loaded: what comes later is another story. */
const UNKNOWN_LOAD_CAP_MS = 15_000;

/** The moments a view says it was loading until, in milliseconds. */
const anchorsOf = (view: ViewFacts): number[] => [...view.marks.map((mark) => mark.at), ...(view.loading === null ? [] : [view.loading])];

/**
 * The axis of a view: it ends where the page had finished loading. A view that says no moment of its
 * own is framed by the requests it made first.
 */
export function axisFor(view: ViewFacts, resources: readonly WaterfallResource[]): Axis {
  const anchors = anchorsOf(view);

  if (anchors.length > 0) return axisOf(Math.max(...anchors));

  const early = resources.filter((resource) => resource.start < UNKNOWN_LOAD_CAP_MS);

  return axisOf(Math.min(UNKNOWN_LOAD_CAP_MS, Math.max(1_000, ...early.map((resource) => resource.start + resource.duration))));
}

/** How long after it began a view's requests are read: until the load had ended, else a quarter of a minute. */
export function readUntil(view: ViewFacts): number {
  const anchors = anchorsOf(view);

  return anchors.length > 0 ? axisOf(Math.max(...anchors)).max : UNKNOWN_LOAD_CAP_MS;
}

/** How far along the axis a moment lies, as a share of it, kept inside it. */
export const shareOf = (at: number, axis: Axis): number => Math.min(1, Math.max(0, at / axis.max));

/** About how wide the track of the chart is until it has been measured, in pixels. */
const TRACK_PX = 340;
const LABEL_PX_PER_CHAR = 7;
const LABEL_PAD_PX = 14;

/**
 * The line each name of a moment stands in, so that names close together are not set over each other:
 * each takes the first line it fits on, left to right. A name that fits on none takes the line whose
 * last name ends soonest. `chars` is how long the name is, and `track` how wide the room is, in pixels.
 */
export function placeLabels(items: ReadonlyArray<{ share: number; chars: number }>, lines: number, track: number = TRACK_PX): number[] {
  const ends = Array.from({ length: lines }, () => -Infinity);
  const uses = Array.from({ length: lines }, () => 0);

  return items.map((item) => {
    const width = item.chars * LABEL_PX_PER_CHAR + LABEL_PAD_PX;
    // A name at the start stands to the right of its line, one at the end to the left of it, the others about it.
    const left = item.share < 0.1 ? item.share * track : item.share > 0.88 ? item.share * track - width : item.share * track - width / 2;
    const free = ends.findIndex((end) => left >= end + 4);
    // Where none is free, the line that is clear soonest, and of those the one that has the fewest names.
    const crowded = ends.map((end, line) => ({ end, line })).sort((a, b) => a.end - b.end || (uses[a.line] as number) - (uses[b.line] as number))[0] as { line: number };
    const line = free >= 0 ? free : crowded.line;

    ends[line] = left + width;
    uses[line] = (uses[line] as number) + 1;

    return line;
  });
}

// ---------------------------------------------------------------------------------------------
// What is drawn.

export type ShowFilter = 'all' | 'blocking' | 'slow' | 'failed';

export const SHOW_FILTERS: readonly ShowFilter[] = ['all', 'blocking', 'slow', 'failed'];

export interface Filters {
  /** A resource type, or all of them. */
  type: string;
  show: ShowFilter;
}

const shows: Record<ShowFilter, (resource: WaterfallResource) => boolean> = {
  all: () => true,
  blocking: (resource) => resource.blocking,
  slow: (resource) => resource.slow,
  failed: (resource) => resource.failed,
};

export interface Drawn {
  /** The resources that began before the axis ended, and meet the filters, by the time they began. */
  rows: WaterfallResource[];
  /** The requests of the view that began after the axis ended. */
  late: number;
}

export function drawn(resources: readonly WaterfallResource[], axis: Axis, filters: Filters): Drawn {
  const early = resources.filter((resource) => resource.start < axis.max);

  return {
    rows: early.filter((resource) => (filters.type === '' || resource.type === filters.type) && shows[filters.show](resource)),
    late: resources.length - early.length,
  };
}

/** How many rows are shown until the reader asks for the rest. */
export const FIRST_ROWS = 15;

/** The duration that most of these resources came in under: the 80th percentile, to a figure a person would say. */
export function mostCameInUnder(resources: readonly WaterfallResource[]): number | null {
  if (resources.length === 0) return null;

  const sorted = resources.map((resource) => resource.duration).sort((a, b) => a - b);
  const value = sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * 0.8) - 1)] as number;
  const step = value < 100 ? 10 : value < 1_000 ? 100 : 500;

  return Math.max(step, Math.ceil(value / step) * step);
}

/** The types the view's resources are of, the commonest first, for the filter. */
export function typesOf(resources: readonly WaterfallResource[]): string[] {
  const counted = new Map<string, number>();

  for (const resource of resources) counted.set(resource.type, (counted.get(resource.type) ?? 0) + 1);

  return [...counted].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([type]) => type);
}

// ---------------------------------------------------------------------------------------------
// What is asked.

/** The most resources or long tasks of one view that are read. */
export const RESOURCE_LIMIT = 500;
export const TASK_LIMIT = 200;

/** Two seconds either side of the load: the requests that belong to it are not dated to the millisecond. */
const PADDING_MICROS = 2_000_000;

export function resourcesQuery(view: ViewFacts): RumSearchParams {
  return {
    type: 'resources',
    from: view.start - PADDING_MICROS,
    to: view.start + readUntil(view) * 1_000 + PADDING_MICROS,
    filters: [{ field: 'view.id', op: '=', value: view.id }],
    columns: [...RESOURCE_COLUMNS],
    limit: RESOURCE_LIMIT,
  };
}

export function tasksQuery(view: ViewFacts): RumSearchParams {
  return {
    type: 'long_tasks',
    from: view.start - PADDING_MICROS,
    to: view.start + readUntil(view) * 1_000 + PADDING_MICROS,
    filters: [{ field: 'view.id', op: '=', value: view.id }],
    columns: [...TASK_COLUMNS],
    limit: TASK_LIMIT,
  };
}

/** A request or a long task found by its id, over the seconds around where it happened: it is read for the panel of a thing the load did not draw. */
export function focusQuery(type: 'resources' | 'long_tasks', id: string, at: number): RumSearchParams {
  return {
    type,
    from: at - PADDING_MICROS,
    to: at + PADDING_MICROS,
    filters: [{ field: type === 'resources' ? 'resource.id' : 'long_task.id', op: '=', value: id }],
    columns: type === 'resources' ? [...RESOURCE_COLUMNS] : [...TASK_COLUMNS],
    limit: 1,
  };
}
