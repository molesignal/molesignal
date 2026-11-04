import { http } from '@/lib/http';

export interface RunningQuery {
  id: string;
  orgId?: string;
  userId?: string;
  startedAt: number;
  statement: string;
  scannedRows?: number;
  cancelled?: boolean;
}

export async function list(): Promise<RunningQuery[]> {
  const { data } = await http.get<RunningQuery[]>('/query/running');
  return data;
}

export async function cancel(id: string): Promise<void> {
  await http.post(`/query/${encodeURIComponent(id)}/cancel`);
}
