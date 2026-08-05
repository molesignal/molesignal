import { http } from '@/lib/http';

export interface RunningQuery {
  id: string;
  org_id?: string;
  user_id?: string;
  started_at_micros: number;
  statement: string;
  scanned_rows?: number;
  cancelled?: boolean;
}

export async function list(): Promise<RunningQuery[]> {
  const { data } = await http.get<RunningQuery[]>('/query/running');
  return data;
}

export async function cancel(id: string): Promise<void> {
  await http.post(`/query/${encodeURIComponent(id)}/cancel`);
}
