import type { Page } from '@playwright/test';

/**
 * Answers for the two endpoints an explorer asks for what a field holds across
 * everything a query matches: `POST /api/v1/web/logs/field-stats` and
 * `POST /api/v1/web/traces/field-stats`.
 *
 * Neither is answered by default. A spec that does not serve them gets `{}`
 * from the catch-all, which the page reads as an analysis that did not come
 * back: the panel is then of the loaded rows, and says why.
 */

/** What an analysis is asked: the field, and the rows it is read across. */
export interface AskedFieldStats {
  field: string;
  from: number;
  to: number;
  /** Logs: the stream and the query the list sent. */
  stream?: string;
  filters?: unknown[];
  free_text?: string[];
  /** Traces: the traces on the page. */
  trace_ids?: string[];
}

/** The commonest values of a field, with how many rows hold each. */
type Top = Array<[value: string, count: number]>;

interface Held {
  total: number;
  present: number;
  top: Top;
}

const listed = (top: Top) => top.map(([value, count]) => ({ value, count }));

/**
 * The analysis of a text field: `top` are all its values, counted. Where one
 * of them repeats they are also followed over time, each rising to the middle
 * of the range.
 */
export function textFieldStats(asked: AskedFieldStats, { total, present, top }: Held) {
  const bucket = 300_000_000;
  const origin = Math.floor(asked.from / bucket) * bucket;
  const stretches = Math.floor((asked.to - 1 - origin) / bucket) + 1;

  return {
    kind: 'text',
    total,
    present,
    distinct: top.length,
    distinct_exact: true,
    top: listed(top),
    top_skipped: false,
    ...((top[0]?.[1] ?? 0) > 1 && {
      trend: {
        origin_micros: origin,
        bucket_micros: bucket,
        series: top.slice(0, 5).map(([value, count]) => ({
          value,
          counts: Array.from({ length: stretches }, (_, index) =>
            Math.round((count / stretches) * (index < stretches / 2 ? 0.5 : 1.5)),
          ),
        })),
      },
    }),
  };
}

/** The analysis of a number that is a code: a handful of values, measured and counted into bars. */
export function numberFieldStats(_asked: AskedFieldStats, { total, present, top }: Held) {
  const values = top.map(([value]) => Number(value));
  const [min, max] = [Math.min(...values), Math.max(...values)];
  const mean = top.reduce((sum, [value, count]) => sum + Number(value) * count, 0) / present;
  const width = 20;
  const origin = Math.floor(min / width) * width;
  const counts = Array<number>(Math.floor((max - origin) / width) + 1).fill(0);

  for (const [value, count] of top) counts[Math.floor((Number(value) - origin) / width)]! += count;

  return {
    kind: 'number',
    total,
    present,
    distinct: top.length,
    distinct_exact: true,
    range: { min: String(min), max: String(max) },
    spread: { avg: mean, q1: min, median: min, q3: max, stddev_samp: 118.2, stddev_pop: 118.1, skewness: 1.52, kurtosis: 0.41 },
    bars: { origin, width, counts },
    top: listed(top),
    top_skipped: false,
  };
}

type Answer = (asked: AskedFieldStats) => Record<string, unknown> | { status: number };

async function serve(page: Page, url: string, answer: Answer): Promise<AskedFieldStats[]> {
  const asked: AskedFieldStats[] = [];

  await page.route(url, async (route) => {
    const body = route.request().postDataJSON() as AskedFieldStats;
    const answered = answer(body);

    asked.push(body);
    await route.fulfill(
      typeof answered.status === 'number' ? { status: answered.status, json: {} } : { json: answered },
    );
  });

  return asked;
}

/**
 * Answers what a log field holds across every row the query matches. Register
 * it after the routes of the list. Returns what was asked, in the order it was.
 */
export function serveLogFieldStats(page: Page, answer: Answer): Promise<AskedFieldStats[]> {
  return serve(page, '**/api/v1/web/logs/field-stats', answer);
}

/** The same for a field of a Span, across every Span of the traces on the page. */
export function serveTraceFieldStats(page: Page, answer: Answer): Promise<AskedFieldStats[]> {
  return serve(page, '**/api/v1/web/traces/field-stats', answer);
}
