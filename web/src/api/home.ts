import type { StreamType } from '@/api/streams';
import { http } from '@/lib/http';

export type HomeHealthStatus = 'healthy' | 'degraded' | 'delayed' | 'no_data' | 'unknown';

export interface HomeOverviewWindow {
  start_micros: number;
  end_micros: number;
  window_secs: number;
}

export interface HomeStatsProbe {
  succeeded: number;
  total: number;
}

export interface HomeStreamOverview {
  id: string;
  name: string;
  stream_type: Exclude<StreamType, 'extend'>;
  status: HomeHealthStatus;
  rows: number;
  stored_bytes: number;
  first_received_at_micros: number | null;
  last_received_at_micros: number | null;
}

export interface HomeSignalOverview {
  stream_type: Exclude<StreamType, 'extend'>;
  status: HomeHealthStatus;
  total_streams: number;
  active_streams: number;
  rows: number;
  stored_bytes: number;
  last_received_at_micros: number | null;
}

export interface HomeOverviewBucket {
  start_micros: number;
  end_micros: number;
  ingested_bytes: number | null;
  stored_bytes: number;
  rows: number;
}

export interface HomeOverview {
  generated_at_micros: number;
  window: HomeOverviewWindow;
  ingest_status: HomeHealthStatus;
  probe_reason: string | null;
  ingested_bytes: number | null;
  stored_bytes: number;
  rows: number;
  compression_savings_ratio: number | null;
  active_streams: number;
  total_streams: number;
  attention_streams: number;
  last_received_at_micros: number | null;
  stats_probe: HomeStatsProbe;
  buckets: HomeOverviewBucket[];
  signals: HomeSignalOverview[];
  streams: HomeStreamOverview[];
}

export async function overview(params: {
  windowSecs: number;
  bucketCount?: number;
}): Promise<HomeOverview> {
  const { data } = await http.get<HomeOverview>('/home/overview', {
    params: {
      window_secs: params.windowSecs,
      bucket_count: params.bucketCount,
    },
  });
  return data;
}
