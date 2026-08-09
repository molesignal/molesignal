import { http } from '@/lib/http';

export interface IntakeResult {
  accepted: number;
  rejected: number;
  errors?: Array<{ index: number; reason: string }>;
}

export async function intakeLogs(stream: string, events: unknown[]): Promise<IntakeResult> {
  const { data } = await http.post<IntakeResult>(`/intake/logs/${stream}`, events);
  return data;
}

export async function intakeMetrics(stream: string, events: unknown[]): Promise<IntakeResult> {
  const { data } = await http.post<IntakeResult>(`/intake/metrics/${stream}`, events);
  return data;
}

export async function intakeTraces(stream: string, events: unknown[]): Promise<IntakeResult> {
  const { data } = await http.post<IntakeResult>(`/intake/traces/${stream}`, events);
  return data;
}
