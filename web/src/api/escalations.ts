import { http } from '@/lib/http';
import type { EscalationPolicy } from '@/types/alerting';

/** Write payload — the backend derives `id`/`org_id` from the path/context. */
export type EscalationPolicyInput = Pick<
  EscalationPolicy,
  'name' | 'steps' | 'repeat' | 'max_loops'
>;

export async function list(): Promise<EscalationPolicy[]> {
  const { data } = await http.get<EscalationPolicy[]>('/alerts/escalations');
  return data;
}
export async function get(id: string): Promise<EscalationPolicy> {
  const { data } = await http.get<EscalationPolicy>(`/alerts/escalations/${id}`);
  return data;
}
export async function create(p: EscalationPolicyInput): Promise<EscalationPolicy> {
  const { data } = await http.post<EscalationPolicy>('/alerts/escalations', p);
  return data;
}
export async function update(id: string, p: EscalationPolicyInput): Promise<EscalationPolicy> {
  const { data } = await http.put<EscalationPolicy>(`/alerts/escalations/${id}`, p);
  return data;
}
export async function remove(id: string) {
  await http.delete(`/alerts/escalations/${id}`);
}
