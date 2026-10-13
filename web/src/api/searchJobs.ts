import { http } from '@/lib/http';

export const SEARCH_JOB_STATES = ['pending', 'running', 'done', 'failed', 'cancelled'] as const;

export type SearchJobState = (typeof SEARCH_JOB_STATES)[number];

/** A query somebody submitted, or the backfill of a data pipeline. */
export type SearchJobKind = 'query' | 'backfill';

export interface SearchJobSubmitter {
  display_name: string;
  email: string;
}

/** The query a job runs, as it was submitted. Times are epoch microseconds. */
export interface SearchJobRequest {
  language: 'sql' | 'promql';
  statement: string;
  time_range: { start: number; end: number };
  stream: { name: string; stream_type: string } | null;
  limit: number | null;
}

export interface SearchJob {
  job_id: string;
  state: SearchJobState;
  kind: SearchJobKind;
  /** The pipeline a backfill belongs to; null for a query. */
  pipeline_id: string | null;
  /** The generation of the execution: 0 at first, one higher after every retry. */
  attempt: number;
  user_id: string;
  /** Null once the user record is gone. */
  submitted_by: SearchJobSubmitter | null;
  /** Null when the stored request cannot be decoded. */
  request: SearchJobRequest | null;
  submitted_at_micros: number;
  started_at_micros: number | null;
  finished_at_micros: number | null;
  result_object_key: string | null;
  result_rows: number | null;
  error: string | null;
  expires_at_micros: number;
}

export type SearchJobSummary = Record<SearchJobState, number> & { total: number };

export interface SearchJobResultPage {
  state: SearchJobState;
  result_rows: number | null;
  page: number;
  page_size: number;
  rows: unknown[];
  has_more: boolean;
  error: string | null;
}

export interface SearchJobListParams {
  /** Only jobs in these states; all of them when empty or left out. */
  states?: readonly SearchJobState[];
  limit?: number;
  /** How many of the most recent jobs to skip. */
  offset?: number;
}

const jobPath = (id: string) => `/query/jobs/${encodeURIComponent(id)}`;

export async function get(id: string): Promise<SearchJob> {
  const { data } = await http.get<SearchJob>(jobPath(id));
  return data;
}

/** The organization's jobs, most recently submitted first. */
export async function list(params: SearchJobListParams = {}): Promise<SearchJob[]> {
  const { data } = await http.get<SearchJob[]>('/query/jobs', {
    params: {
      limit: params.limit ?? 50,
      ...(params.offset ? { offset: params.offset } : {}),
      ...(params.states?.length ? { state: params.states.join(',') } : {}),
    },
  });
  return data;
}

/** How many jobs the organization has in each state, whatever the list is filtered to. */
export async function summary(): Promise<SearchJobSummary> {
  const { data } = await http.get<SearchJobSummary>('/query/jobs/summary');
  return data;
}

export async function cancel(id: string): Promise<SearchJob> {
  const { data } = await http.post<SearchJob>(`${jobPath(id)}/cancel`);
  return data;
}

export async function retry(id: string): Promise<SearchJob> {
  const { data } = await http.post<SearchJob>(`${jobPath(id)}/retry`);
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(jobPath(id));
}

/** One page of the rows of a finished job; `pageSize` is at most 10 000. */
export async function results(
  id: string,
  page: number,
  pageSize: number,
): Promise<SearchJobResultPage> {
  const { data } = await http.get<SearchJobResultPage>(`${jobPath(id)}/results`, {
    params: { page, page_size: pageSize },
  });
  return data;
}
