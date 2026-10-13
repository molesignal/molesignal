/**
 * Mock search jobs: the queries that run in the background and the backfills
 * of data pipelines, as `/query/jobs` reports them. One job of every state,
 * submitted by the people of the same made-up company as the other mocks.
 *
 * The list lives in the mock server of one test, so what a test cancels,
 * retries or deletes is gone with it. A test moves a job along the way a
 * worker would through `updateMockSearchJob`, and swaps the whole list through
 * `setMockSearchJobs`.
 *
 * A job belongs to whoever submitted it. The mock answers as the backend does
 * for an administrator, who is shown everybody's jobs, until a test names a
 * member with `setMockSearchJobViewer`: that member is shown their own, and
 * anybody else's job answers as one that does not exist.
 *
 * Every time is relative to the mock session's frozen clock.
 */
import type { Express, Request, Response } from 'express';

import {
  SEARCH_JOB_STATES,
  type SearchJob,
  type SearchJobState,
} from '../../src/api/searchJobs';

const SECOND = 1_000_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export const MOCK_SEARCH_JOB = {
  running: '2yW8mNqK4tRc7vLpXe1zHb3Ad0F',
  pendingBackfill: '2yW8pT5sJh9QxUoVn3aYkCe6Gr1',
  done: '2yVzR1dMw7BfKq0LsEi4HnPu8Tc',
  failed: '2yVtC6gXa2NjOy5WbDk9ZmSv3Fe',
  cancelled: '2ySqH4uLe8PzIr1MxAo7GtYc5Nb',
  doneEmpty: '2yPnB9kVf3JwTs6DhQy2ElXa0Zm',
} as const;

/** The jobs the mock organization starts with, most recently submitted first. */
export function seedSearchJobs(nowMicros: number): SearchJob[] {
  return [
    {
      job_id: MOCK_SEARCH_JOB.pendingBackfill,
      state: 'pending',
      kind: 'backfill',
      pipeline_id: 'checkout-logs',
      attempt: 0,
      user_id: 'u2',
      submitted_by: { display_name: 'Kenji Watanabe', email: 'kenji@example.com' },
      request: {
        language: 'sql',
        statement: 'SELECT * FROM logs',
        time_range: { start: nowMicros - 7 * DAY, end: nowMicros },
        stream: { name: 'logs', stream_type: 'logs' },
        limit: null,
      },
      submitted_at_micros: nowMicros - MINUTE,
      started_at_micros: null,
      finished_at_micros: null,
      result_object_key: null,
      result_rows: null,
      error: null,
      expires_at_micros: nowMicros - MINUTE + 7 * DAY,
    },
    {
      job_id: MOCK_SEARCH_JOB.running,
      state: 'running',
      kind: 'query',
      pipeline_id: null,
      attempt: 0,
      user_id: 'u1',
      submitted_by: { display_name: 'Dana Reyes', email: 'sre@example.com' },
      request: {
        language: 'sql',
        statement:
          'SELECT status, count(*) AS requests\nFROM "access_logs"\nWHERE status >= 500\nGROUP BY status',
        time_range: { start: nowMicros - DAY, end: nowMicros },
        stream: { name: 'access_logs', stream_type: 'logs' },
        limit: 100_000,
      },
      submitted_at_micros: nowMicros - 2 * MINUTE,
      started_at_micros: nowMicros - 110 * SECOND,
      finished_at_micros: null,
      result_object_key: null,
      result_rows: null,
      error: null,
      expires_at_micros: nowMicros - 2 * MINUTE + 7 * DAY,
    },
    {
      job_id: MOCK_SEARCH_JOB.done,
      state: 'done',
      kind: 'query',
      pipeline_id: null,
      attempt: 0,
      user_id: 'u1',
      submitted_by: { display_name: 'Dana Reyes', email: 'sre@example.com' },
      request: {
        language: 'promql',
        statement: 'sum by (service) (rate(http_requests_total[5m]))',
        time_range: { start: nowMicros - 3 * DAY, end: nowMicros - 3 * HOUR },
        stream: { name: 'http_requests_total', stream_type: 'metrics' },
        limit: null,
      },
      submitted_at_micros: nowMicros - 3 * HOUR,
      started_at_micros: nowMicros - 3 * HOUR + 2 * SECOND,
      finished_at_micros: nowMicros - 3 * HOUR + 44 * SECOND,
      result_object_key: `search_jobs/v1/acme-prod/${MOCK_SEARCH_JOB.done}-0.ndjson`,
      result_rows: 12_480,
      error: null,
      expires_at_micros: nowMicros - 3 * HOUR + 7 * DAY,
    },
    {
      job_id: MOCK_SEARCH_JOB.failed,
      state: 'failed',
      kind: 'query',
      pipeline_id: null,
      attempt: 1,
      user_id: 'u3',
      submitted_by: { display_name: 'Amara Okafor', email: 'amara@example.com' },
      request: {
        language: 'sql',
        statement:
          "SELECT trace_id, duration FROM traces WHERE service_name = 'checkout' ORDER BY duration DESC",
        time_range: { start: nowMicros - 30 * DAY, end: nowMicros - 5 * HOUR },
        stream: { name: 'traces', stream_type: 'traces' },
        limit: 50_000,
      },
      submitted_at_micros: nowMicros - 5 * HOUR,
      started_at_micros: nowMicros - 5 * HOUR + SECOND,
      finished_at_micros: nowMicros - 5 * HOUR + 4 * MINUTE + 12 * SECOND,
      result_object_key: null,
      result_rows: null,
      error:
        'resource exhausted: the query needs 2.4 GiB of memory, more than the 2.0 GiB one search job may use',
      expires_at_micros: nowMicros - 5 * HOUR + 7 * DAY,
    },
    {
      job_id: MOCK_SEARCH_JOB.cancelled,
      state: 'cancelled',
      kind: 'query',
      pipeline_id: null,
      attempt: 0,
      // Somebody who has left since: the user record is gone, the id is all that remains.
      user_id: 'u-former-7c41',
      submitted_by: null,
      request: {
        language: 'sql',
        statement: 'SELECT actor, action, count(*) FROM audit GROUP BY actor, action',
        time_range: { start: nowMicros - 90 * DAY, end: nowMicros - 26 * HOUR },
        stream: { name: 'audit', stream_type: 'logs' },
        limit: null,
      },
      submitted_at_micros: nowMicros - 26 * HOUR,
      started_at_micros: nowMicros - 26 * HOUR + SECOND,
      finished_at_micros: nowMicros - 26 * HOUR + 38 * SECOND,
      result_object_key: null,
      result_rows: null,
      error: null,
      expires_at_micros: nowMicros - 26 * HOUR + 7 * DAY,
    },
    {
      job_id: MOCK_SEARCH_JOB.doneEmpty,
      state: 'done',
      kind: 'query',
      pipeline_id: null,
      attempt: 0,
      user_id: 'u2',
      submitted_by: { display_name: 'Kenji Watanabe', email: 'kenji@example.com' },
      request: {
        language: 'sql',
        statement: "SELECT * FROM logs WHERE trace_id = '4bf92f3577b34da6a3ce929d0e0e4736'",
        time_range: { start: nowMicros - 4 * DAY, end: nowMicros - 2 * DAY },
        stream: { name: 'logs', stream_type: 'logs' },
        limit: 1_000,
      },
      submitted_at_micros: nowMicros - 2 * DAY,
      started_at_micros: nowMicros - 2 * DAY + SECOND,
      finished_at_micros: nowMicros - 2 * DAY + SECOND + 820_000,
      result_object_key: `search_jobs/v1/acme-prod/${MOCK_SEARCH_JOB.doneEmpty}-0.ndjson`,
      result_rows: 0,
      error: null,
      expires_at_micros: nowMicros - 2 * DAY + 7 * DAY,
    },
  ];
}

export function registerSearchJobRoutes(app: Express, nowIso: string): void {
  const nowMicros = Date.parse(nowIso) * 1000;
  let jobs = seedSearchJobs(nowMicros);
  // The member the caller is; null for an administrator.
  let viewer: string | null = null;
  const visible = (job: SearchJob) => viewer === null || job.user_id === viewer;
  const find = (id: string | undefined) =>
    jobs.find((job) => job.job_id === id && visible(job));
  const missing = (res: Response) => res.status(404).json({ error: 'not found: row not found' });

  app.get('/api/v1/query/jobs', (req: Request, res: Response) => {
    const names = String(req.query.state ?? '')
      .split(',')
      .map((name) => name.trim())
      .filter(Boolean);
    const unknown = names.find(
      (name) => !SEARCH_JOB_STATES.includes(name as SearchJobState),
    );
    if (unknown) {
      return res
        .status(400)
        .json({ error: `invalid argument: unknown search job state \`${unknown}\`` });
    }

    const limit = Math.min(1000, Math.max(1, Number(req.query.limit ?? 100) || 100));
    const offset = Math.max(0, Number(req.query.offset ?? 0) || 0);
    const matching = jobs
      .filter(visible)
      .filter((job) => names.length === 0 || names.includes(job.state))
      .sort((a, b) => b.submitted_at_micros - a.submitted_at_micros);
    return res.json(matching.slice(offset, offset + limit));
  });

  // Before `/:id`, which would otherwise take "summary" for a job.
  app.get('/api/v1/query/jobs/summary', (_req, res) => {
    const shown = jobs.filter(visible);
    const counts = Object.fromEntries(
      SEARCH_JOB_STATES.map((state) => [
        state,
        shown.filter((job) => job.state === state).length,
      ]),
    );
    res.json({ ...counts, total: shown.length });
  });

  app.get('/api/v1/query/jobs/:id', (req, res) => {
    const job = find(req.params.id);
    return job ? res.json(job) : missing(res);
  });

  app.get('/api/v1/query/jobs/:id/results', (req, res) => {
    const job = find(req.params.id);
    if (!job) return missing(res);

    const page = Math.max(1, Number(req.query.page ?? 1) || 1);
    const pageSize = Math.min(10_000, Math.max(1, Number(req.query.page_size ?? 1000) || 1000));
    const total = job.state === 'done' ? (job.result_rows ?? 0) : 0;
    const first = (page - 1) * pageSize;
    const rows = Array.from({ length: Math.max(0, Math.min(pageSize, total - first)) }, (_, i) => ({
      row: first + i + 1,
      service: ['checkout', 'gateway', 'payments'][(first + i) % 3],
      value: (first + i) % 97,
    }));
    return res.json({
      state: job.state,
      result_rows: job.result_rows,
      page,
      page_size: pageSize,
      rows,
      has_more: page * pageSize < total,
      error: job.error,
    });
  });

  app.post('/api/v1/query/jobs/:id/cancel', (req, res) => {
    const job = find(req.params.id);
    if (!job) return missing(res);
    if (job.state !== 'pending' && job.state !== 'running' && job.state !== 'cancelled') {
      return res
        .status(400)
        .json({ error: `invalid argument: search job in state \`${job.state}\` cannot be cancelled` });
    }
    job.state = 'cancelled';
    job.error = null;
    job.finished_at_micros = nowMicros;
    return res.json(job);
  });

  app.post('/api/v1/query/jobs/:id/retry', (req, res) => {
    const job = find(req.params.id);
    if (!job) return missing(res);
    if (job.state !== 'failed' && job.state !== 'cancelled') {
      return res
        .status(400)
        .json({ error: `invalid argument: search job in state \`${job.state}\` cannot be retried` });
    }
    const ttl = job.expires_at_micros - job.submitted_at_micros;
    Object.assign(job, {
      state: 'pending',
      attempt: job.attempt + 1,
      result_object_key: null,
      result_rows: null,
      error: null,
      submitted_at_micros: nowMicros,
      started_at_micros: null,
      finished_at_micros: null,
      expires_at_micros: nowMicros + ttl,
    } satisfies Partial<SearchJob>);
    return res.json(job);
  });

  app.delete('/api/v1/query/jobs/:id', (req, res) => {
    if (!find(req.params.id)) return missing(res);
    jobs = jobs.filter((job) => job.job_id !== req.params.id);
    return res.json({ deleted: true });
  });

  // What a test uses to stand in for the worker, for other people's jobs, and for who is asking.
  app.put('/api/v1/__mock/search-jobs', (req: Request, res: Response) => {
    jobs = Array.isArray(req.body) ? (req.body as SearchJob[]) : [];
    res.json({ jobs: jobs.length });
  });
  app.put('/api/v1/__mock/search-jobs/viewer', (req: Request, res: Response) => {
    const userId = (req.body as { user_id?: unknown }).user_id;
    viewer = typeof userId === 'string' ? userId : null;
    res.json({ viewer });
  });
  app.post('/api/v1/__mock/search-jobs/:id', (req: Request, res: Response) => {
    // The worker changes a job whoever is looking at the list.
    const job = jobs.find((candidate) => candidate.job_id === req.params.id);
    if (!job) return missing(res);
    Object.assign(job, req.body as Partial<SearchJob>);
    return res.json(job);
  });
}

async function send(port: number, method: 'PUT' | 'POST', path: string, body: unknown) {
  const response = await fetch(`http://127.0.0.1:${port}/api/v1/__mock/search-jobs${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!response.ok) throw new Error(`mock search jobs were not changed: ${response.status}`);
}

/**
 * Makes the caller a member who submitted jobs as `userId`, so that only those are shown;
 * `null` makes the caller an administrator again.
 */
export function setMockSearchJobViewer(port: number, userId: string | null): Promise<void> {
  return send(port, 'PUT', '/viewer', { user_id: userId });
}

/** Replaces the organization's jobs, for a test about an empty or a long list. */
export function setMockSearchJobs(port: number, jobs: readonly SearchJob[]): Promise<void> {
  return send(port, 'PUT', '', jobs);
}

/** Changes one job the way the worker would, for a test about a job that moves on. */
export function updateMockSearchJob(
  port: number,
  id: string,
  change: Partial<SearchJob>,
): Promise<void> {
  return send(port, 'POST', `/${encodeURIComponent(id)}`, change);
}
