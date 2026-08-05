import { http } from '@/lib/http';

export interface SystemHealth {
  status: 'ok' | 'degraded';
  reason?: string;
}

export async function get(): Promise<SystemHealth> {
  const { data } = await http.get<SystemHealth>('/healthz', {
    validateStatus: (status) => status === 200 || status === 503,
  });
  return data;
}
