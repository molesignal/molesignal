import { http } from '@/lib/http';
import type { IncidentRouting, Priority, Severity, TimeCondition } from '@/types/alerting';

/** Where an incident came from, as a routing rule can narrow it. */
export type IncidentOrigin = 'rule' | 'external' | 'synthetic' | 'manual';

export const INCIDENT_ORIGINS: readonly IncidentOrigin[] = ['rule', 'external', 'synthetic', 'manual'];

/** How a condition compares a label: one value, any of a list, or a whole-value pattern. */
export type ConditionOperator = 'equals' | 'not_equals' | 'in' | 'matches';

export const CONDITION_OPERATORS: readonly ConditionOperator[] = ['equals', 'not_equals', 'in', 'matches'];

export interface LabelCondition {
  key: string;
  operator: ConditionOperator;
  values: string[];
}

/** Where a matching rule sends an incident: a policy, or a team and its on-call policy. */
export type RoutingTarget = { kind: 'policy'; policyId: string } | { kind: 'team'; teamId: string };

export interface RoutingRule {
  id: string;
  orgId: string;
  name: string;
  enabled: boolean;
  /** Smaller first. */
  position: number;
  conditions: LabelCondition[];
  minSeverity: Severity | null;
  /** Empty: every origin. */
  origins: IncidentOrigin[];
  /** When the incident opens: inside or outside some weekly hours. Absent: any time. */
  time?: TimeCondition | null;
  /** Empty: any priority, none included. */
  priorities?: Priority[];
  target: RoutingTarget;
  createdBy: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface RoutingRuleInput {
  name: string;
  enabled: boolean;
  conditions: LabelCondition[];
  minSeverity: Severity | null;
  origins: IncidentOrigin[];
  time: TimeCondition | null;
  priorities: Priority[];
  target: RoutingTarget;
}

/** A team on the routing page: its on-call policy and the services it owns. */
export interface TeamRouting {
  teamId: string;
  name: string;
  memberCount: number;
  escalationPolicyId: string | null;
  /** `namespace/name` of each service the APM catalog gives it. */
  services: string[];
}

/** What routing would do with a set of labels, and what each step saw. */
export interface RoutingExplanation {
  decision: {
    escalationPolicyId: string;
    teamId: string | null;
    routing: IncidentRouting;
  };
  matchedRuleId: string | null;
  service: string | null;
  serviceTeamId: string | null;
  labelTeamId: string | null;
}

export interface RoutingProbe {
  labels: Record<string, string>;
  services?: string[];
  severity: Severity;
  priority?: Priority | null;
  /** The moment to try (micros); now when absent. */
  at?: number;
}

const base = '/alerts/routing';

export async function listRules(): Promise<RoutingRule[]> {
  const { data } = await http.get<RoutingRule[]>(`${base}/rules`);
  return data;
}

export async function createRule(input: RoutingRuleInput): Promise<RoutingRule> {
  const { data } = await http.post<RoutingRule>(`${base}/rules`, input);
  return data;
}

export async function updateRule(id: string, input: RoutingRuleInput): Promise<RoutingRule> {
  const { data } = await http.put<RoutingRule>(`${base}/rules/${encodeURIComponent(id)}`, input);
  return data;
}

export async function removeRule(id: string): Promise<void> {
  await http.delete(`${base}/rules/${encodeURIComponent(id)}`);
}

export async function reorderRules(ids: string[]): Promise<RoutingRule[]> {
  const { data } = await http.put<RoutingRule[]>(`${base}/rules/order`, { ids });
  return data;
}

export async function listTeams(): Promise<TeamRouting[]> {
  const { data } = await http.get<TeamRouting[]>(`${base}/teams`);
  return data;
}

export async function setTeamPolicy(teamId: string, escalationPolicyId: string | null): Promise<void> {
  await http.put(`${base}/teams/${encodeURIComponent(teamId)}`, { escalationPolicyId: escalationPolicyId });
}

export async function test(probe: RoutingProbe): Promise<RoutingExplanation> {
  const { data } = await http.post<RoutingExplanation>(`${base}/test`, probe);
  return data;
}
