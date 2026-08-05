import { http } from '@/lib/http';

export interface Domain {
  id: string;
  hostname: string;
  state: string;
  cert_not_after_micros?: number;
  last_error?: string;
  created_at_micros: number;
  updated_at_micros: number;
}

export interface CreateDomainPayload {
  hostname: string;
}

export async function list(): Promise<Domain[]> {
  const { data } = await http.get<Domain[]>('/domains');
  return data;
}

export async function create(payload: CreateDomainPayload): Promise<Domain> {
  const { data } = await http.post<Domain>('/domains', payload);
  return data;
}

export async function renew(id: string): Promise<void> {
  await http.post(`/domains/${encodeURIComponent(id)}/renew`);
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/domains/${encodeURIComponent(id)}`);
}
