/**
 * Results for the Logs and Traces explorers.
 *
 * The shared mock (`mockBackend.ts`) lists no stream an explorer can query and
 * no trace, so under it both pages show their empty state. A spec that wants
 * results on the page calls `serveExplorerResults` after `mountMockRoutes`:
 * Playwright asks the route registered last first, so these answers stand in
 * front of the shared mock's. Once the page is open, `expectExplorerResults`
 * holds when the results are on it.
 *
 * Nothing here is random. What the two streams hold is worked out from the
 * mock session's frozen clock, so every run lists the same rows and counts
 * the same bars.
 */
import { expect, type Page } from '@playwright/test';

import { serveLogCounts, serveTraceCounts } from './histogramCounts';
import { FROZEN_NOW_ISO } from './mockBackend';

const MINUTE = 60_000_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;

/** Something a stream holds: when it happened, whether it counts as an error, and its row in the list. */
interface Held {
  micros: number;
  error: boolean;
  row: Record<string, unknown>;
}

const field = (name: string, dataType = 'utf8') => ({
  name,
  data_type: dataType,
  nullable: name !== '_timestamp',
  indexed: false,
});

/** A field the stream keeps an index of: the field list of an explorer sets those apart. */
const indexed = (name: string, dataType = 'utf8') => ({ ...field(name, dataType), indexed: true });

const stream = (
  id: string,
  name: string,
  type: 'logs' | 'traces',
  fields: Array<ReturnType<typeof field>>,
) => ({
  id,
  name,
  stream_type: type,
  schema: { fields },
  retention: { days: 7 },
  effective_retention: { days: 7 },
  created_at_micros: NOW - 30 * DAY,
  updated_at_micros: NOW - DAY,
});

const STREAMS = [
  stream('application-logs', 'application_logs', 'logs', [
    indexed('_timestamp', 'timestamp'),
    indexed('level'),
    indexed('service.name'),
    field('message'),
    indexed('trace_id'),
    field('http.status_code', 'int64'),
    field('duration_ms', 'float64'),
  ]),
  stream('traces-default', 'default', 'traces', [
    // The four a new trace stream indexes; `name` is one this stream was given an index for.
    indexed('trace_id'),
    indexed('span_id'),
    indexed('parent_span_id'),
    indexed('name'),
    field('kind'),
    field('start_time_unix_nano', 'int64'),
    field('end_time_unix_nano', 'int64'),
    field('duration_ns', 'int64'),
    field('status_code'),
    indexed('service.name'),
    field('http.request.method'),
    field('http.response.status_code', 'int64'),
    field('http.route'),
  ]),
];

/** `count` moments spread evenly over the `slot` that ended `slotsAgo` slots before now, the latest first. */
const spread = (count: number, slot: number, slotsAgo: number) =>
  Array.from({ length: count }, (_, nth) => NOW - slotsAgo * slot - Math.round(((nth + 0.5) * slot) / count));

/** A trace id for the nth trace: the same 32 digits on every run, others for every trace. */
const traceId = (nth: number) =>
  ((BigInt(nth + 1) * 0x9e3779b97f4a7c15f39cc0605cedc835n) % (1n << 128n)).toString(16).padStart(32, '0');

interface LogLine {
  level: string;
  service: string;
  message: string;
  /** A line about a request also says how it was answered, and carries the request's trace. */
  request?: { status: number; ms: number };
}

const line = (level: string, service: string, message: string, request?: LogLine['request']): LogLine => ({
  level,
  service,
  message,
  ...(request && { request }),
});

const GATEWAY_TIMEOUT = line(
  'ERROR',
  'payment-gateway',
  'upstream timeout after 3000 ms calling https://acquirer.example/v2/authorise (attempt 3 of 3)',
  { status: 504, ms: 3000 },
);

/** What the services say, each line in its turn. */
const LOG_LINES = [
  line('INFO', 'checkout-api', 'POST /api/orders 201 in 182 ms', { status: 201, ms: 182 }),
  line('INFO', 'catalog', 'GET /api/products/akita-hoodie 200 in 41 ms', { status: 200, ms: 41 }),
  line('WARN', 'payment-gateway', 'card declined by issuer: insufficient funds'),
  line('INFO', 'checkout-api', 'order 48213 confirmed, receipt queued for delivery'),
  line('DEBUG', 'inventory', 'stock cache refreshed: 1204 items in 38 ms'),
  line('INFO', 'web', 'GET / 200 in 12 ms', { status: 200, ms: 12 }),
  GATEWAY_TIMEOUT,
  line('INFO', 'inventory', 'stock reserved for order 48213: 3 items'),
];

/**
 * The last hour of the log stream, the latest line first: a handful of lines a
 * minute, and for the six minutes a deploy went wrong many more, most of them
 * the gateway timing out.
 */
const LOG_EVENTS: Held[] = Array.from({ length: 60 }, (_, minutesAgo) => {
  const failing = minutesAgo >= 18 && minutesAgo < 24;

  return spread(failing ? 14 : 4 + (minutesAgo % 3), MINUTE, minutesAgo).map((micros, nth) => ({
    micros,
    timedOut: failing && nth % 3 !== 0,
  }));
})
  .flat()
  .map(({ micros, timedOut }, index) => {
    const said = timedOut ? GATEWAY_TIMEOUT : LOG_LINES[index % LOG_LINES.length]!;

    return {
      micros,
      error: said.level === 'ERROR',
      row: {
        _timestamp: micros,
        level: said.level,
        'service.name': said.service,
        message: said.message,
        ...(said.request && {
          trace_id: traceId(index),
          'http.status_code': said.request.status,
          duration_ms: said.request.ms,
        }),
      },
    };
  });

/**
 * How many traces began in each of the last 24 hours, the latest hour first:
 * a busy afternoon, then a quiet night.
 */
const TRACES_PER_HOUR = [11, 10, 8, 6, 4, 3, 2, 2, 2, 3, 4, 6, 7, 9, 10, 12, 13, 15, 16, 16, 15, 14, 13, 12];

const TRACE_ROOTS = [
  { operation: 'POST /api/orders', service: 'checkout-api' },
  { operation: 'GET /api/products/:id', service: 'catalog' },
  { operation: 'POST /api/payments', service: 'payment-gateway' },
  { operation: 'GET /api/cart', service: 'checkout-api' },
  { operation: 'inventory.reserve', service: 'inventory' },
  { operation: 'GET /', service: 'web' },
];
const TRACE_DURATIONS_MS = [182, 41, 2450, 96, 12, 640, 318];

/** The last day of traces, the latest first. One in five has a span that failed. */
const TRACES: Held[] = TRACES_PER_HOUR.flatMap((count, hoursAgo) => spread(count, HOUR, hoursAgo)).map(
  (micros, index) => {
    const errors = index % 5 === 3 ? 1 + (index % 2) : 0;

    return {
      micros,
      error: errors > 0,
      row: {
        trace_id: traceId(index),
        ...TRACE_ROOTS[index % TRACE_ROOTS.length]!,
        start_ns: micros * 1_000,
        duration_ms: TRACE_DURATIONS_MS[index % TRACE_DURATIONS_MS.length]!,
        span_count: 3 + ((index * 5) % 11),
        error_count: errors,
      },
    };
  },
);

/** What a list endpoint is asked: the body of a POST, or the query of a GET, where numbers arrive as text. */
type Ask = Record<string, unknown>;

/**
 * Answers a list endpoint over `held`, a page at a time: a range to begin
 * with, then the cursor of another page of it, each as long as was asked. A
 * cursor is the place its page starts at, among what the range matched.
 * Filters and sort orders are not applied: the list is everything held in the
 * range, latest first.
 */
function lister(held: readonly Held[]) {
  let matched = held;

  return (ask: Ask) => {
    if (!ask.cursor) {
      const from = Number(ask.from ?? -Infinity);
      const to = Number(ask.to ?? Infinity);
      matched = held.filter(({ micros }) => micros >= from && micros < to);
    }

    const limit = Number(ask.limit ?? matched.length);
    const start = Number(ask.cursor ?? 0);
    const end = start + limit;

    return {
      items: matched.slice(start, end).map(({ row }) => row),
      next_cursor: end < matched.length ? String(end) : null,
      previous_cursor: start > 0 ? String(Math.max(0, start - limit)) : null,
      has_more: end < matched.length,
    };
  };
}

/**
 * Gives the Logs and Traces explorers something to show: a log stream with an
 * hour of lines and a trace stream with a day of traces. Each is listed a page
 * at a time, and its histogram counts all of it.
 */
export async function serveExplorerResults(page: Page): Promise<void> {
  const logs = lister(LOG_EVENTS);
  const traces = lister(TRACES);

  await page.route(/\/api\/v1\/streams(\?.*)?$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: STREAMS }) : route.fallback(),
  );
  await page.route('**/api/v1/web/logs', (route) => route.fulfill({ json: logs(route.request().postDataJSON()) }));
  await page.route(/\/api\/v1\/web\/traces(\?.*)?$/, (route) =>
    route.fulfill({ json: traces(Object.fromEntries(new URL(route.request().url()).searchParams)) }),
  );
  // Counted from what the lists page through, so a histogram and its list agree.
  await serveLogCounts(page, { events: LOG_EVENTS });
  await serveTraceCounts(page, { events: TRACES });
}

/** The explorers `serveExplorerResults` gives results to. */
export type Explorer = 'logs' | 'traces';

/**
 * A row of each explorer's list. A trace has no mark of its own: it is a
 * button of the results pane that says whether it is the trace that is open.
 */
export const EXPLORER_ROW: Record<Explorer, string> = {
  logs: '[data-log-result-row="logs"]',
  traces: '[data-workspace-pane="trace-results"] button[aria-pressed]',
};

/** A field each stream holds, as its explorer's field list shows it. */
const EXPLORER_FIELD: Record<Explorer, string> = {
  logs: '[data-log-field-row="level"]',
  traces: '[data-trace-field="trace_id"]',
};

/** Room for a page to load and list: a load from the dev server takes several seconds while a suite runs. */
const LISTED = { timeout: 15_000 };

/**
 * Holds once an explorer shows what `serveExplorerResults` serves it: a row of
 * its list, a field the rows carry, and the histogram of every match. A
 * network gone quiet does not tell that, since the list is drawn after it.
 * Nothing here is read from the copy, so it holds in any language.
 */
export async function expectExplorerResults(page: Page, explorer: Explorer): Promise<void> {
  await expect(page.locator(EXPLORER_ROW[explorer]).first()).toBeVisible(LISTED);
  await expect(page.locator(EXPLORER_FIELD[explorer])).toBeVisible();
  await expect(page.locator('[data-explorer-module="histogram"]')).toHaveAttribute(
    'data-histogram-counting',
    'all',
  );
}
