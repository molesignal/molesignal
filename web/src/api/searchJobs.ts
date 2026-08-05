import { http } from '@/lib/http';

export interface SearchJob {
  job_id: string;
  state: string;
  submitted_at_micros: number;
  started_at_micros: number | null;
  finished_at_micros: number | null;
  result_object_key: string | null;
  result_rows: number | null;
  error: string | null;
  expires_at_micros: number;
}

export async function get(id: string): Promise<SearchJob> {
  const { data } = await http.get<SearchJob>(`/query/jobs/${encodeURIComponent(id)}`);
  return data;
}

export async function list(limit = 50): Promise<SearchJob[]> {
  const { data } = await http.get<SearchJob[]>('/query/jobs', { params: { limit } });
  return data;
}
