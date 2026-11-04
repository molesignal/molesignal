import type { Page } from '@playwright/test';

import { explorerRead, explorerReadBody } from './explorerReads';

/**
 * Answers for the two endpoints an explorer histogram asks for the count of
 * everything a query matches: `GET /api/v1/web/logs/histogram` (a POST when
 * the query is too long for an address) and `GET /api/v1/web/traces/histogram`.
 *
 * Neither is answered by default. A spec that lists rows of its own and does
 * not serve their count gets `{}` from the catch-all, which the page reads as
 * a count that did not come back: the histogram is then of the loaded rows,
 * and says under itself that everything could not be counted. A spec that
 * wants the histogram of every match serves it with one of these.
 */

/** What a bar holds, by its place among the bars that were asked for; `null` leaves it empty. */
export type PerBar = (index: number, bars: number) => { total: number; errors: number } | null;

/** One thing that happened: when, in microseconds, and whether it is an error. */
export interface CountedEvent {
  micros: number;
  error?: boolean;
}

/**
 * What a histogram endpoint is told to answer:
 * - a function says what each of the bars asked for holds;
 * - `events` are counted into the bars as a backend holding exactly them would, so the
 *   histogram agrees with a list that serves the same rows;
 * - `unavailable` answers no counts, and why (`time_budget`, `busy`, `scan_limit`,
 *   `timestamp_masked`).
 */
export type Counts = PerBar | { events: CountedEvent[] } | { unavailable: string };

/** The bars a chart asks for: each `bucket_time` long, the first beginning at `origin_time`. */
export interface AskedBars {
  from: number;
  to: number;
  bucketTime: number;
  originTime: number;
}

/** What a histogram endpoint answers to the bars it was asked for. */
export function countsAnswer(asked: AskedBars, counts: Counts) {
  const answer = { bucketTime: asked.bucketTime, originTime: asked.originTime };
  if (typeof counts !== 'function' && 'unavailable' in counts) {
    return { ...answer, buckets: [], split: false, ...counts };
  }

  const bars = Math.ceil((asked.to - asked.originTime) / asked.bucketTime);
  const held = typeof counts === 'function' ? counts : eventsPerBar(asked, counts.events);
  const buckets = Array.from({ length: bars }, (_, index) => {
    const bar = held(index, bars);

    return bar && bar.total > 0
      ? [{ startTime: asked.originTime + index * asked.bucketTime, ...bar }]
      : [];
  }).flat();

  return { ...answer, buckets, split: true };
}

/** The events inside the range that was asked, counted by the bar each falls in. */
function eventsPerBar(asked: AskedBars, events: CountedEvent[]): PerBar {
  const held = new Map<number, { total: number; errors: number }>();

  for (const event of events) {
    if (event.micros < asked.from || event.micros >= asked.to) continue;

    const index = Math.floor((event.micros - asked.originTime) / asked.bucketTime);
    const bar = held.get(index) ?? { total: 0, errors: 0 };
    held.set(index, { total: bar.total + 1, errors: bar.errors + (event.error ? 1 : 0) });
  }

  return (index) => held.get(index) ?? null;
}

/** What a Logs histogram asks: the query the list sent, and the bars to count it into. */
export type AskedLogCounts = AskedBars & { stream: string; filters: unknown[]; freeText: string[] };

/**
 * Answers the count of every log row the query matches. Register it after the
 * routes of the list. Returns what was asked, in the order it was asked.
 */
export async function serveLogCounts(page: Page, counts: Counts): Promise<AskedLogCounts[]> {
  const asked: AskedLogCounts[] = [];
  await page.route(explorerRead('web/logs/histogram'), async (route) => {
    const body = explorerReadBody<AskedLogCounts>(route.request());
    asked.push(body);
    await route.fulfill({ json: countsAnswer(body, counts) });
  });

  return asked;
}

/** The same for traces: the count of every trace the list query matches, by when it began. */
export async function serveTraceCounts(page: Page, counts: Counts): Promise<URLSearchParams[]> {
  const asked: URLSearchParams[] = [];
  await page.route(/\/api\/v1\/web\/traces\/histogram(\?.*)?$/, async (route) => {
    const params = new URL(route.request().url()).searchParams;
    asked.push(params);
    await route.fulfill({
      json: countsAnswer(
        {
          from: Number(params.get('from')),
          to: Number(params.get('to')),
          bucketTime: Number(params.get('bucketTime')),
          originTime: Number(params.get('originTime')),
        },
        counts,
      ),
    });
  });

  return asked;
}
