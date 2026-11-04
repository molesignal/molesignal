import { http } from '@/lib/http';

/**
 * What a quota or throttle step of a realtime pipeline let through and held
 * back today (UTC), counted by every node. Nodes write every few seconds.
 */
export interface LimitBucket {
  step: string;
  /** Empty for a quota that is not split and for a throttle; the values it is split by as a JSON list otherwise, `*` past the cap. */
  bucket: string;
  admittedEvents: number;
  admittedBytes: number;
  overEvents: number;
  overBytes: number;
}

export interface PipelineLimits {
  dayStart: number;
  buckets: LimitBucket[];
}

export async function today(pipelineId: string): Promise<PipelineLimits> {
  const { data } = await http.get<PipelineLimits>(
    `/scheduled_pipelines/${encodeURIComponent(pipelineId)}/limits`,
  );
  return data;
}
