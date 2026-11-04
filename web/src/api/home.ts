import type { StreamType } from '@/api/streams';
import { http } from '@/lib/http';

export type HomeHealthStatus = 'healthy' | 'degraded' | 'delayed' | 'no_data' | 'unknown';

export interface HomeOverviewWindow {
  startTime: number;
  endTime: number;
  windowSecs: number;
}

export interface HomeStatsProbe {
  succeeded: number;
  total: number;
}

export interface HomeStreamOverview {
  id: string;
  name: string;
  streamType: Exclude<StreamType, 'extend'>;
  status: HomeHealthStatus;
  rows: number;
  storedBytes: number;
  firstReceivedAt: number | null;
  lastReceivedAt: number | null;
}

export interface HomeSignalOverview {
  streamType: Exclude<StreamType, 'extend'>;
  status: HomeHealthStatus;
  totalStreams: number;
  activeStreams: number;
  rows: number;
  storedBytes: number;
  lastReceivedAt: number | null;
}

export interface HomeOverviewBucket {
  startTime: number;
  endTime: number;
  intakeBytes: number | null;
  storedBytes: number;
  rows: number;
}

export interface HomeOverview {
  generatedAt: number;
  window: HomeOverviewWindow;
  intakeStatus: HomeHealthStatus;
  probeReason: string | null;
  intakeBytes: number | null;
  storedBytes: number;
  rows: number;
  compressionSavingsRatio: number | null;
  activeStreams: number;
  totalStreams: number;
  attentionStreams: number;
  lastReceivedAt: number | null;
  statsProbe: HomeStatsProbe;
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
      windowSecs: params.windowSecs,
      bucketCount: params.bucketCount,
    },
  });
  return data;
}
