import { http } from '@/lib/http';
import type { AlertRule } from '@/types/alerting';

export type AlertRuleInput = Omit<AlertRule, 'id' | 'org_id' | 'escalation_policy_id'> & {
  escalation_policy_id?: string;
};

export async function list(): Promise<AlertRule[]> {
  const { data } = await http.get<AlertRule[]>('/alerts/rules');
  return data;
}
export async function get(id: string): Promise<AlertRule> {
  const { data } = await http.get<AlertRule>(`/alerts/rules/${id}`);
  return data;
}
export async function create(rule: AlertRuleInput): Promise<AlertRule> {
  const { data } = await http.post<AlertRule>('/alerts/rules', rule);
  return data;
}
export async function update(id: string, rule: AlertRuleInput): Promise<AlertRule> {
  const { data } = await http.put<AlertRule>(`/alerts/rules/${id}`, rule);
  return data;
}
export async function remove(id: string) {
  await http.delete(`/alerts/rules/${id}`);
}
