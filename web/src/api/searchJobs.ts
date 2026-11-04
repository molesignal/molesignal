import { http } from '@/lib/http';

export const SEARCH_JOB_STATES = ['pending', 'running', 'done', 'failed', 'cancelled'] as const;

export type SearchJobState = (typeof SEARCH_JOB_STATES)[number];

/** A query somebody submitted, or the backfill of a data pipeline. */
export type SearchJobKind = 'query' | 'backfill';

export interface SearchJobSubmitter {
  displayName: string;
  email: string;
}

/** The query a job runs, as it was submitted. Times are epoch microseconds. */
export interface SearchJobRequest {
  language: 'sql' | 'promql';
  statement: string;
  timeRange: { start: number; end: number };
  stream: { name: string; streamType: string } | null;
  limit: number | null;
}

export interface SearchJob {
  jobId: string;
  state: SearchJobState;
  kind: SearchJobKind;
  /** The pipeline a backfill belongs to; null for a query. */
  pipelineId: string | null;
  /** The generation of the execution: 0 at first, one higher after every retry. */
  attempt: number;
  userId: string;
  /** Null once the user record is gone. */
  submittedBy: SearchJobSubmitter | null;
  /** Null when the stored request cannot be decoded. */
  request: SearchJobRequest | null;
  submittedAt: number;
  startedAt: number | null;
  finishedAt: number | null;
  resultObjectKey: string | null;
  resultRows: number | null;
  error: string | null;
  expiresAt: number;
}

export type SearchJobSummary = Record<SearchJobState, number> & { total: number };

export interface SearchJobResultPage {
  state: SearchJobState;
  resultRows: number | null;
  page: number;
  pageSize: number;
  rows: unknown[];
  hasMore: boolean;
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
    params: { page, pageSize: pageSize },
  });
  return data;
}
