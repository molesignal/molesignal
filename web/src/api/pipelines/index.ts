import { http } from '@/lib/http';

export interface ScheduledPipeline {
  id: string;
  org_id?: string;
  name: string;
  description?: string;
  source_stream?: string;
  target_stream?: string;
  function_steps?: unknown;
  cron?: string;
  lookback_secs?: number;
  enabled?: boolean;
  last_run_at_micros?: number;
  last_run_state?: 'running' | 'succeeded' | 'failed' | 'cancelled' | string;
  last_run_started_at_micros?: number;
  last_run_finished_at_micros?: number;
  last_run_scanned_rows?: number;
  last_run_error?: string;
  runs_24h?: number;
  succeeded_runs_24h?: number;
  failed_runs_24h?: number;
  created_at_micros?: number;
  updated_at_micros?: number;
  [key: string]: unknown;
}

export interface PipelineInput {
  name: string;
  source_stream: string;
  target_stream: string;
  function_steps: unknown;
  cron: string;
  lookback_secs?: number;
  enabled?: boolean;
}

export async function list(): Promise<ScheduledPipeline[]> {
  const { data } = await http.get<ScheduledPipeline[] | { items: ScheduledPipeline[] }>(
    '/scheduled_pipelines',
  );
  return Array.isArray(data) ? data : data.items;
}

export async function get(id: string): Promise<ScheduledPipeline> {
  const { data } = await http.get<ScheduledPipeline>(
    `/scheduled_pipelines/${encodeURIComponent(id)}`,
  );
  return data;
}

export async function create(payload: PipelineInput): Promise<ScheduledPipeline> {
  const { data } = await http.post<ScheduledPipeline>('/scheduled_pipelines', payload);
  return data;
}

export async function update(id: string, payload: PipelineInput): Promise<ScheduledPipeline> {
  const { data } = await http.put<ScheduledPipeline>(
    `/scheduled_pipelines/${encodeURIComponent(id)}`,
    payload,
  );
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/scheduled_pipelines/${encodeURIComponent(id)}`);
}
