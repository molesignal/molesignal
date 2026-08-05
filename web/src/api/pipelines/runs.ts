import { http } from '@/lib/http';

export interface PipelineRun {
  id: string;
  pipeline_id: string;
  state: 'running' | 'succeeded' | 'failed' | 'cancelled' | string;
  started_at_micros: number;
  finished_at_micros: number | null;
  scanned_rows: number;
  error: string | null;
}

export interface BackfillSubmissionInput {
  start_micros: number;
  end_micros: number;
}

export interface BackfillResp {
  job_id: string;
  monitor: string;
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
