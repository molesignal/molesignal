import { http } from '@/lib/http';

export interface ScheduledPipeline {
  id: string;
  orgId?: string;
  name: string;
  description?: string;
  sourceStream?: string;
  targetStream?: string;
  functionSteps?: unknown;
  cron?: string;
  lookbackSecs?: number;
  enabled?: boolean;
  lastRunAt?: number;
  /** Per source, the time before which its events have been handled (microseconds). */
  readUntil?: Record<string, number>;
  /** The soonest planned retry of a stretch of time that failed. */
  retry?: PipelineRetry | null;
  /** An alert pipeline's count of today's events (UTC), with the latest drop and failure. */
  alertRecord?: AlertPipelineRecord | null;
  /** Whose access a pipeline that reads `_audit` reads it with: whoever last saved it. */
  auditReaderId?: string | null;
  /** Their name and email; `null` when the user no longer exists. */
  auditReader?: { id: string; displayName: string; email: string } | null;
  lastRunState?: 'running' | 'succeeded' | 'failed' | 'cancelled' | string;
  lastRunStartedAt?: number;
  lastRunFinishedAt?: number;
  lastRunScannedRows?: number;
  lastRunError?: string;
  runs24h?: number;
  succeededRuns24h?: number;
  failedRuns24h?: number;
  createdAt?: number;
  updatedAt?: number;
  [key: string]: unknown;
}

export interface PipelineRetry {
  source: string;
  /** The attempt the retry will be. */
  attempt: number;
  nextAt: number;
  error: string;
}

/** What became of the events an alert pipeline saw today (UTC). */
export interface AlertPipelineRecord {
  day?: string;
  processed?: number;
  dropped?: number;
  failed?: number;
  lastDropped?: AlertPipelineOutcome;
  lastFailed?: AlertPipelineOutcome;
}

export interface AlertPipelineOutcome {
  atTime: number;
  /** The rule or source the event came from. */
  rule: string;
  reason?: string | null;
}

export interface PipelineInput {
  name: string;
  sourceStream: string;
  targetStream: string;
  functionSteps: unknown;
  cron: string;
  lookbackSecs?: number;
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
