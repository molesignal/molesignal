import * as queryApi from '@/api/query';
import type { SavedView } from '@/api/savedViews';
import * as streamsApi from '@/api/streams';
import { resolveWindow, type TimeWindow } from '@/stores/useTimeStore';

/** The most rows a report prints: as many as the explorer loads when the view is opened there. */
export const REPORT_ROW_LIMIT = 200;

/** The span of time a report covers, in microseconds since the epoch. */
export interface ReportRange {
  start: number;
  end: number;
}

export interface SavedViewResult {
  columns: string[];
  rows: unknown[][];
  /** The statement returns more rows than the report prints. */
  truncated: boolean;
}

/** A SQL view that names no stream, in an organization with no logs stream to open it on. */
export class NoStreamError extends Error {
  constructor() {
    super('the saved view names no stream and there is no logs stream to query');
    this.name = 'NoStreamError';
  }
}

/**
 * The range a report of this view covers: the one the report states, or, when
 * the report states none, the view's own look-back ending when the report is made.
 *
 * Throws when the stated range cannot be read.
 */
export function reportRange(
  view: SavedView,
  stated: TimeWindow | undefined,
  now: Date,
): ReportRange {
  if (stated) {
    const { from, to } = resolveWindow(stated, now);

    return { start: from.getTime() * 1_000, end: to.getTime() * 1_000 };
  }

  const end = now.getTime() * 1_000;

  return { start: end - view.time_range_secs * 1_000_000, end };
}

/**
 * Runs a saved view over a range, along the path the explorer takes when the
 * view is opened there: the view's own statement, sent to `POST /query`.
 */
export async function runSavedView(
  view: SavedView,
  orgId: string,
  range: ReportRange,
): Promise<SavedViewResult> {
  if (view.language === 'promql') {
    // A range query: the engine samples each series evenly to fit the row limit.
    const result = await queryApi.runQuery({
      org_id: orgId,
      language: 'promql',
      statement: view.statement,
      time_range: range,
      limit: REPORT_ROW_LIMIT,
    });

    return { columns: result.columns, rows: result.rows, truncated: false };
  }

  const result = await queryApi.runQuery({
    org_id: orgId,
    language: 'sql',
    statement: view.statement,
    time_range: range,
    stream: { name: await sqlStream(view), stream_type: 'logs' },
    // One row more than is printed says whether the statement returns more.
    limit: REPORT_ROW_LIMIT + 1,
  });

  return {
    columns: result.columns,
    rows: result.rows.slice(0, REPORT_ROW_LIMIT),
    truncated: result.rows.length > REPORT_ROW_LIMIT,
  };
}

/**
 * The stream a SQL statement is sent with. The engine requires one; a view
 * saved without it is opened on the first logs stream, as the explorer does.
 */
async function sqlStream(view: SavedView): Promise<string> {
  const named = view.stream?.trim();
  if (named) return named;

  const streams = await streamsApi.list(500);
  const first = streams.find((stream) => stream.type === 'logs' && streamsApi.isQueryable(stream));
  if (!first) throw new NoStreamError();

  return first.name;
}
