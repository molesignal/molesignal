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
      jobId: MOCK_SEARCH_JOB.pendingBackfill,
      state: 'pending',
      kind: 'backfill',
      pipelineId: 'checkout-logs',
      attempt: 0,
      userId: 'u2',
      submittedBy: { displayName: 'Kenji Watanabe', email: 'kenji@example.com' },
      request: {
        language: 'sql',
        statement: 'SELECT * FROM logs',
        timeRange: { start: nowMicros - 7 * DAY, end: nowMicros },
        stream: { name: 'logs', streamType: 'logs' },
        limit: null,
      },
      submittedAt: nowMicros - MINUTE,
      startedAt: null,
      finishedAt: null,
      resultObjectKey: null,
      resultRows: null,
      error: null,
      expiresAt: nowMicros - MINUTE + 7 * DAY,
    },
    {
      jobId: MOCK_SEARCH_JOB.running,
      state: 'running',
      kind: 'query',
      pipelineId: null,
      attempt: 0,
      userId: 'u1',
      submittedBy: { displayName: 'Dana Reyes', email: 'sre@example.com' },
      request: {
        language: 'sql',
        statement:
          'SELECT status, count(*) AS requests\nFROM "access_logs"\nWHERE status >= 500\nGROUP BY status',
        timeRange: { start: nowMicros - DAY, end: nowMicros },
        stream: { name: 'access_logs', streamType: 'logs' },
        limit: 100_000,
      },
      submittedAt: nowMicros - 2 * MINUTE,
      startedAt: nowMicros - 110 * SECOND,
      finishedAt: null,
      resultObjectKey: null,
      resultRows: null,
      error: null,
      expiresAt: nowMicros - 2 * MINUTE + 7 * DAY,
    },
    {
      jobId: MOCK_SEARCH_JOB.done,
      state: 'done',
      kind: 'query',
      pipelineId: null,
      attempt: 0,
      userId: 'u1',
      submittedBy: { displayName: 'Dana Reyes', email: 'sre@example.com' },
      request: {
        language: 'promql',
        statement: 'sum by (service) (rate(http_requests_total[5m]))',
        timeRange: { start: nowMicros - 3 * DAY, end: nowMicros - 3 * HOUR },
        stream: { name: 'http_requests_total', streamType: 'metrics' },
        limit: null,
      },
      submittedAt: nowMicros - 3 * HOUR,
      startedAt: nowMicros - 3 * HOUR + 2 * SECOND,
      finishedAt: nowMicros - 3 * HOUR + 44 * SECOND,
      resultObjectKey: `search_jobs/v1/acme-prod/${MOCK_SEARCH_JOB.done}-0.ndjson`,
      resultRows: 12_480,
      error: null,
      expiresAt: nowMicros - 3 * HOUR + 7 * DAY,
    },
    {
      jobId: MOCK_SEARCH_JOB.failed,
      state: 'failed',
      kind: 'query',
      pipelineId: null,
      attempt: 1,
      userId: 'u3',
      submittedBy: { displayName: 'Amara Okafor', email: 'amara@example.com' },
      request: {
        language: 'sql',
        statement:
          "SELECT trace_id, duration FROM traces WHERE service_name = 'checkout' ORDER BY duration DESC",
        timeRange: { start: nowMicros - 30 * DAY, end: nowMicros - 5 * HOUR },
        stream: { name: 'traces', streamType: 'traces' },
        limit: 50_000,
      },
      submittedAt: nowMicros - 5 * HOUR,
      startedAt: nowMicros - 5 * HOUR + SECOND,
      finishedAt: nowMicros - 5 * HOUR + 4 * MINUTE + 12 * SECOND,
      resultObjectKey: null,
      resultRows: null,
      error:
        'resource exhausted: the query needs 2.4 GiB of memory, more than the 2.0 GiB one search job may use',
      expiresAt: nowMicros - 5 * HOUR + 7 * DAY,
    },
    {
      jobId: MOCK_SEARCH_JOB.cancelled,
      state: 'cancelled',
      kind: 'query',
      pipelineId: null,
      attempt: 0,
      // Somebody who has left since: the user record is gone, the id is all that remains.
      userId: 'u-former-7c41',
      submittedBy: null,
      request: {
        language: 'sql',
        statement: 'SELECT actor, action, count(*) FROM audit GROUP BY actor, action',
        timeRange: { start: nowMicros - 90 * DAY, end: nowMicros - 26 * HOUR },
        stream: { name: 'audit', streamType: 'logs' },
        limit: null,
      },
      submittedAt: nowMicros - 26 * HOUR,
      startedAt: nowMicros - 26 * HOUR + SECOND,
      finishedAt: nowMicros - 26 * HOUR + 38 * SECOND,
      resultObjectKey: null,
      resultRows: null,
      error: null,
      expiresAt: nowMicros - 26 * HOUR + 7 * DAY,
    },
    {
      jobId: MOCK_SEARCH_JOB.doneEmpty,
      state: 'done',
      kind: 'query',
      pipelineId: null,
      attempt: 0,
      userId: 'u2',
      submittedBy: { displayName: 'Kenji Watanabe', email: 'kenji@example.com' },
      request: {
        language: 'sql',
        statement: "SELECT * FROM logs WHERE trace_id = '4bf92f3577b34da6a3ce929d0e0e4736'",
        timeRange: { start: nowMicros - 4 * DAY, end: nowMicros - 2 * DAY },
        stream: { name: 'logs', streamType: 'logs' },
        limit: 1_000,
      },
      submittedAt: nowMicros - 2 * DAY,
      startedAt: nowMicros - 2 * DAY + SECOND,
      finishedAt: nowMicros - 2 * DAY + SECOND + 820_000,
      resultObjectKey: `search_jobs/v1/acme-prod/${MOCK_SEARCH_JOB.doneEmpty}-0.ndjson`,
      resultRows: 0,
      error: null,
      expiresAt: nowMicros - 2 * DAY + 7 * DAY,
    },
  ];
}

export function registerSearchJobRoutes(app: Express, nowIso: string): void {
  const nowMicros = Date.parse(nowIso) * 1000;
  let jobs = seedSearchJobs(nowMicros);
  // The member the caller is; null for an administrator.
  let viewer: string | null = null;
  const visible = (job: SearchJob) => viewer === null || job.userId === viewer;
  const find = (id: string | undefined) =>
    jobs.find((job) => job.jobId === id && visible(job));
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
      .sort((a, b) => b.submittedAt - a.submittedAt);
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
    const pageSize = Math.min(10_000, Math.max(1, Number(req.query.pageSize ?? 1000) || 1000));
    const total = job.state === 'done' ? (job.resultRows ?? 0) : 0;
    const first = (page - 1) * pageSize;
    const rows = Array.from({ length: Math.max(0, Math.min(pageSize, total - first)) }, (_, i) => ({
      row: first + i + 1,
      service: ['checkout', 'gateway', 'payments'][(first + i) % 3],
      value: (first + i) % 97,
    }));
    return res.json({
      state: job.state,
      resultRows: job.resultRows,
      page,
      pageSize: pageSize,
      rows,
      hasMore: page * pageSize < total,
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
    job.finishedAt = nowMicros;
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
    const ttl = job.expiresAt - job.submittedAt;
    Object.assign(job, {
      state: 'pending',
      attempt: job.attempt + 1,
      resultObjectKey: null,
      resultRows: null,
      error: null,
      submittedAt: nowMicros,
      startedAt: null,
      finishedAt: null,
      expiresAt: nowMicros + ttl,
    } satisfies Partial<SearchJob>);
    return res.json(job);
  });

  app.delete('/api/v1/query/jobs/:id', (req, res) => {
    if (!find(req.params.id)) return missing(res);
    jobs = jobs.filter((job) => job.jobId !== req.params.id);
    return res.json({ deleted: true });
  });

  // What a test uses to stand in for the worker, for other people's jobs, and for who is asking.
  app.put('/api/v1/__mock/search-jobs', (req: Request, res: Response) => {
    jobs = Array.isArray(req.body) ? (req.body as SearchJob[]) : [];
    res.json({ jobs: jobs.length });
  });
  app.put('/api/v1/__mock/search-jobs/viewer', (req: Request, res: Response) => {
    const userId = (req.body as { userId?: unknown }).userId;
    viewer = typeof userId === 'string' ? userId : null;
    res.json({ viewer });
  });
  app.post('/api/v1/__mock/search-jobs/:id', (req: Request, res: Response) => {
    // The worker changes a job whoever is looking at the list.
    const job = jobs.find((candidate) => candidate.jobId === req.params.id);
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
  return send(port, 'PUT', '/viewer', { userId: userId });
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
