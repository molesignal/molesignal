import { http } from '@/lib/http';

export interface IngestResult {
  accepted: number;
  rejected: number;
  errors?: Array<{ index: number; reason: string }>;
}

export async function ingestLogs(stream: string, events: unknown[]): Promise<IngestResult> {
  const { data } = await http.post<IngestResult>(`/ingest/logs/${stream}`, events);
  return data;
}

export async function ingestMetrics(stream: string, events: unknown[]): Promise<IngestResult> {
  const { data } = await http.post<IngestResult>(`/ingest/metrics/${stream}`, events);
  return data;
}

export async function ingestTraces(stream: string, events: unknown[]): Promise<IngestResult> {
  const { data } = await http.post<IngestResult>(`/ingest/traces/${stream}`, events);
  return data;
}
