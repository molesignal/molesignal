import { http } from '@/lib/http';

/** One run of a scheduled pipeline. Every event it read ends written, dropped or failed. */
export interface PipelineRun {
  id: string;
  pipeline_id: string;
  state: 'running' | 'succeeded' | 'failed' | 'cancelled' | string;
  started_at_micros: number;
  finished_at_micros: number | null;
  /** Events read from the sources. */
  scanned_rows: number;
  /** Events that went everywhere they were meant to. */
  written_rows?: number;
  /** Events a step or the dedupe dropped on purpose. */
  dropped_rows?: number;
  /** Events a step failed on, a stream rejected or a connector did not take. */
  failed_rows?: number;
  /** 1 for a first attempt; more when the run tried a failed stretch of time again. */
  attempt?: number;
  /** Why the run failed. */
  error: string | null;
  /** What went wrong without failing the run. */
  warning?: string | null;
}

export interface BackfillSubmissionInput {
  start_micros: number;
  end_micros: number;
}

export interface BackfillJob {
  /** The source this job reads. */
  source: string;
  job_id: string;
  monitor: string;
}

/** One job per source the pipeline reads. */
export interface BackfillResp {
  jobs: BackfillJob[];
}

export async function list(
  pipelineId: string,
  before_micros?: number,
  limit = 50,
): Promise<PipelineRun[]> {
  const params: Record<string, number> = { limit };
  if (before_micros !== undefined) params.before_micros = before_micros;
  const { data } = await http.get<PipelineRun[]>(
    `/scheduled_pipelines/${encodeURIComponent(pipelineId)}/runs`,
    { params },
  );
  return data;
}

export async function submitBackfill(
  pipelineId: string,
  input: BackfillSubmissionInput,
): Promise<BackfillResp> {
  const { data } = await http.post<BackfillResp>(
    `/scheduled_pipelines/${encodeURIComponent(pipelineId)}/backfill`,
    input,
  );
  return data;
}
