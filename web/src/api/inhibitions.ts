import { http } from '@/lib/http';
import type { InhibitRule, Severity } from '@/types/alerting';

/** Create/update payload — the backend sets `id`/`orgId`/`createdBy` and counts `inhibiting`. */
export type InhibitRuleInput = Pick<
  InhibitRule,
  'name' | 'enabled' | 'source' | 'target' | 'equal' | 'moreSevereOnly' | 'waitSecs' | 'lingerSecs' | 'comment'
>;

/** One incident a rule would hold back now, and the one that would hold it back. */
export interface InhibitPair {
  sourceIncidentId: string;
  sourceSummary: string;
  sourceSeverity: Severity;
  targetIncidentId: string;
  targetSummary: string;
  targetSeverity: Severity;
}

const BASE = '/alerts/inhibit-rules';

export async function list(): Promise<InhibitRule[]> {
  const { data } = await http.get<InhibitRule[]>(BASE);
  return data;
}

export async function create(input: InhibitRuleInput): Promise<InhibitRule> {
  const { data } = await http.post<InhibitRule>(BASE, input);
  return data;
}

export async function update(id: string, input: InhibitRuleInput): Promise<InhibitRule> {
  const { data } = await http.put<InhibitRule>(`${BASE}/${encodeURIComponent(id)}`, input);
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`${BASE}/${encodeURIComponent(id)}`);
}

/** What a rule, saved or not, would hold back among the incidents that are active now. */
export async function preview(input: InhibitRuleInput): Promise<InhibitPair[]> {
  const { data } = await http.post<{ pairs: InhibitPair[] }>(`${BASE}/preview`, input);
  return data.pairs;
}
