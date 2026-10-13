import { describe, expect, it } from 'vitest';

import type { SearchJob, SearchJobSummary } from '@/api/searchJobs';

import {
  ACTIVE_REFRESH_MS,
  IDLE_REFRESH_MS,
  attemptNumber,
  filterTotal,
  formatJobDuration,
  hasActiveJobs,
  jobAction,
  jobDurationMicros,
  jobFilterFromParam,
  pageCountFor,
  refreshInterval,
  resultFileName,
  statementLine,
  submitterName,
} from './model';

const SECOND = 1_000_000;

function job(overrides: Partial<SearchJob> = {}): SearchJob {
  return {
    job_id: '2yW8mNqK4tRc7vLpXe1zHb3Ad0F',
    state: 'done',
    kind: 'query',
    pipeline_id: null,
    attempt: 0,
    user_id: 'u1',
    submitted_by: { display_name: 'Dana Reyes', email: 'sre@example.com' },
    request: null,
    submitted_at_micros: 100 * SECOND,
    started_at_micros: 101 * SECOND,
    finished_at_micros: 143 * SECOND,
    result_object_key: null,
    result_rows: null,
    error: null,
    expires_at_micros: 1_000 * SECOND,
    ...overrides,
  };
}

const summary = (counts: Partial<SearchJobSummary>): SearchJobSummary => {
  const all = { pending: 0, running: 0, done: 0, failed: 0, cancelled: 0, ...counts };
  return { ...all, total: all.pending + all.running + all.done + all.failed + all.cancelled };
};

describe('search job filter', () => {
  it('reads a state from the address and takes anything else for all', () => {
    expect(jobFilterFromParam('running')).toBe('running');
    expect(jobFilterFromParam('cancelled')).toBe('cancelled');
    expect(jobFilterFromParam(null)).toBe('all');
    expect(jobFilterFromParam('queued')).toBe('all');
  });

  it('counts what a filter matches from the counts of every state', () => {
    const counts = summary({ pending: 2, running: 1, done: 40, failed: 3 });

    expect(filterTotal('all', counts)).toBe(46);
    expect(filterTotal('done', counts)).toBe(40);
    expect(filterTotal('cancelled', counts)).toBe(0);
  });

  it('offers one page for an empty list and rounds a part page up', () => {
    expect(pageCountFor(0, 50)).toBe(1);
    expect(pageCountFor(50, 50)).toBe(1);
    expect(pageCountFor(51, 50)).toBe(2);
  });
});

describe('search job refresh', () => {
  it('is read every few seconds only while a job waits or runs', () => {
    expect(hasActiveJobs(summary({ pending: 1 }))).toBe(true);
    expect(hasActiveJobs(summary({ running: 2, done: 9 }))).toBe(true);
    expect(hasActiveJobs(summary({ done: 9, failed: 1, cancelled: 3 }))).toBe(false);
    // Before the counts have arrived nothing is known to be under way.
    expect(hasActiveJobs(undefined)).toBe(false);

    expect(refreshInterval(true)).toBe(ACTIVE_REFRESH_MS);
    expect(refreshInterval(false)).toBe(IDLE_REFRESH_MS);
    expect(ACTIVE_REFRESH_MS).toBeLessThan(IDLE_REFRESH_MS);
  });
});

describe('search job row', () => {
  it('offers the one action a state calls for', () => {
    expect(jobAction('pending')).toBe('cancel');
    expect(jobAction('running')).toBe('cancel');
    expect(jobAction('done')).toBe('download');
    expect(jobAction('failed')).toBe('retry');
    expect(jobAction('cancelled')).toBe('retry');
  });

  it('times a job from its start to its end, or to now while it runs', () => {
    expect(jobDurationMicros(job(), 500 * SECOND)).toBe(42 * SECOND);
    expect(
      jobDurationMicros(job({ state: 'running', finished_at_micros: null }), 111 * SECOND),
    ).toBe(10 * SECOND);
    // Waiting in the queue is not running.
    expect(
      jobDurationMicros(
        job({ state: 'pending', started_at_micros: null, finished_at_micros: null }),
        500 * SECOND,
      ),
    ).toBeNull();
    // A job retried from a cancelled run has its times cleared until it starts again.
    expect(
      jobDurationMicros(job({ state: 'cancelled', finished_at_micros: null }), 500 * SECOND),
    ).toBeNull();
    // Two clocks that disagree never make a job run backwards.
    expect(jobDurationMicros(job({ finished_at_micros: 100 * SECOND }), 500 * SECOND)).toBe(0);
  });

  it('writes a duration in the unit it is read in', () => {
    expect(formatJobDuration(null)).toBe('—');
    expect(formatJobDuration(0)).toBe('0 ms');
    expect(formatJobDuration(820_000)).toBe('820 ms');
    expect(formatJobDuration(3_440_000)).toBe('3.4 s');
    expect(formatJobDuration(42 * SECOND)).toBe('42 s');
    expect(formatJobDuration(59_600_000)).toBe('1 min 0 s');
    expect(formatJobDuration(252 * SECOND)).toBe('4 min 12 s');
    expect(formatJobDuration(3_600 * SECOND)).toBe('1 h 0 min');
    expect(formatJobDuration(4_380 * SECOND)).toBe('1 h 13 min');
  });

  it('puts a statement on one line', () => {
    expect(statementLine('SELECT status,\n  count(*)\nFROM  "access_logs"')).toBe(
      'SELECT status, count(*) FROM "access_logs"',
    );
    expect(statementLine(undefined)).toBe('');
  });

  it('names the submitter by name, then email, then id', () => {
    expect(submitterName(job())).toBe('Dana Reyes');
    expect(
      submitterName(job({ submitted_by: { display_name: ' ', email: 'sre@example.com' } })),
    ).toBe('sre@example.com');
    expect(submitterName(job({ submitted_by: null, user_id: 'u-former' }))).toBe('u-former');
  });

  it('counts attempts from one', () => {
    expect(attemptNumber(job())).toBe(1);
    expect(attemptNumber(job({ attempt: 2 }))).toBe(3);
  });

  it('names the file of results after the job, with nothing a file name cannot hold', () => {
    expect(resultFileName(job())).toBe('search-job-2yW8mNqK4tRc7vLpXe1zHb3Ad0F.ndjson');
    expect(resultFileName(job({ job_id: '../a b' }))).toBe('search-job-___a_b.ndjson');
  });
});
