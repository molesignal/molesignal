import { http } from '@/lib/http';
import type { LabelMatcher, MuteRule, MuteWindow } from '@/types/alerting';

/** Create/update payload — the backend sets `id`/`org_id`/`created_by`. */
export interface MuteRuleInput {
  name: string;
  enabled: boolean;
  matchers: LabelMatcher[];
  window: MuteWindow;
  comment: string;
}

export async function list(): Promise<MuteRule[]> {
  const { data } = await http.get<MuteRule[]>('/alerts/mutes');
  return data;
}
export async function get(id: string): Promise<MuteRule> {
  const { data } = await http.get<MuteRule>(`/alerts/mutes/${encodeURIComponent(id)}`);
  return data;
}
export async function create(input: MuteRuleInput): Promise<MuteRule> {
  const { data } = await http.post<MuteRule>('/alerts/mutes', input);
  return data;
}
export async function update(id: string, input: MuteRuleInput): Promise<MuteRule> {
  const { data } = await http.put<MuteRule>(`/alerts/mutes/${encodeURIComponent(id)}`, input);
  return data;
}
export async function remove(id: string): Promise<void> {
  await http.delete(`/alerts/mutes/${encodeURIComponent(id)}`);
}

export async function silenceIncident(
  incidentId: string,
  input: { duration_secs: number; comment: string },
): Promise<MuteRule> {
  const { data } = await http.post<MuteRule>(
    `/alerts/incidents/${encodeURIComponent(incidentId)}/silence`,
    input,
  );
  return data;
}
