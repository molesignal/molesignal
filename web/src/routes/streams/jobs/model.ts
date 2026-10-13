import {
  SEARCH_JOB_STATES,
  type SearchJob,
  type SearchJobState,
  type SearchJobSummary,
} from '@/api/searchJobs';

export type JobFilter = SearchJobState | 'all';

export const JOB_FILTERS: readonly JobFilter[] = ['all', ...SEARCH_JOB_STATES];

/** While a job waits or runs the list is read again every few seconds. */
export const ACTIVE_REFRESH_MS = 3_000;
/** With nothing under way it is still read now and then, so a job submitted elsewhere shows up. */
export const IDLE_REFRESH_MS = 30_000;

export const JOB_PAGE_SIZES = [20, 50, 100];
export const DEFAULT_JOB_PAGE_SIZE = 50;

/** The most rows one request for results may ask for. */
export const RESULT_PAGE_SIZE = 10_000;

export function jobFilterFromParam(value: string | null): JobFilter {
  return JOB_FILTERS.find((filter) => filter === value) ?? 'all';
}

export function isActiveState(state: SearchJobState): boolean {
  return state === 'pending' || state === 'running';
}

/** Whether any job of the organization still waits or runs. */
export function hasActiveJobs(summary: SearchJobSummary | undefined): boolean {
  return summary !== undefined && summary.pending + summary.running > 0;
}

export function refreshInterval(active: boolean): number {
  return active ? ACTIVE_REFRESH_MS : IDLE_REFRESH_MS;
}

/** How many jobs the filter matches. */
export function filterTotal(filter: JobFilter, summary: SearchJobSummary): number {
  return filter === 'all' ? summary.total : summary[filter];
}

export function pageCountFor(total: number, pageSize: number): number {
  return Math.max(1, Math.ceil(total / pageSize));
}

/** What a row offers for a job in this state, besides deleting it. */
export function jobAction(state: SearchJobState): 'cancel' | 'retry' | 'download' {
  if (isActiveState(state)) return 'cancel';
  return state === 'done' ? 'download' : 'retry';
}

/**
 * How long the job has run: from its start to its end, or to now while it is
 * still running. Null for a job that has not started.
 */
export function jobDurationMicros(job: SearchJob, nowMicros: number): number | null {
  if (!job.started_at_micros) return null;
  const end = job.finished_at_micros ?? (job.state === 'running' ? nowMicros : null);
  if (end === null) return null;
  return Math.max(0, end - job.started_at_micros);
}

export function formatJobDuration(micros: number | null): string {
  if (micros === null) return '—';
  const millis = Math.round(micros / 1000);
  if (millis < 1000) return `${millis} ms`;
  if (millis < 10_000) return `${(millis / 1000).toFixed(1)} s`;
  const seconds = Math.round(millis / 1000);
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min ${seconds % 60} s`;
  const hours = Math.floor(minutes / 60);
  return `${hours} h ${minutes % 60} min`;
}

/** A statement on one line: what a list has room for. */
export function statementLine(statement: string | undefined): string {
  return (statement ?? '').replace(/\s+/g, ' ').trim();
}

/** Who submitted the job, by the best name there is for them. */
export function submitterName(job: SearchJob): string {
  return job.submitted_by?.display_name.trim() || job.submitted_by?.email.trim() || job.user_id;
}

/** The attempt a person would count: the first run is attempt 1. */
export function attemptNumber(job: SearchJob): number {
  return job.attempt + 1;
}

export function resultFileName(job: SearchJob): string {
  return `search-job-${job.job_id.replace(/[^A-Za-z0-9_-]/g, '_')}.ndjson`;
}
