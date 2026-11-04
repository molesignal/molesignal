import { http } from '@/lib/http';
import type { AlertRule } from '@/types/alerting';

export type AlertRuleInput = Omit<AlertRule, 'id' | 'orgId' | 'escalationPolicyId'> & {
  escalationPolicyId?: string;
};

export async function list(): Promise<AlertRule[]> {
  const { data } = await http.get<AlertRule[]>('/alerts/rules');
  return data;
}
export async function get(id: string): Promise<AlertRule> {
  const { data } = await http.get<AlertRule>(`/alerts/rules/${id}`);
  return data;
}
/** Where a heartbeat rule is checked in; given only when it is first made or reset. */
export interface HeartbeatAddress {
  path: string;
  /** Set when the instance knows its external address; otherwise the page's own origin is used. */
  url: string | null;
}

/** A saved rule; a heartbeat rule's first save carries its check-in address. */
export type SavedAlertRule = AlertRule & { heartbeatAddress?: HeartbeatAddress };

export async function create(rule: AlertRuleInput): Promise<SavedAlertRule> {
  const { data } = await http.post<SavedAlertRule>('/alerts/rules', rule);
  return data;
}
export async function update(id: string, rule: AlertRuleInput): Promise<SavedAlertRule> {
  const { data } = await http.put<SavedAlertRule>(`/alerts/rules/${id}`, rule);
  return data;
}

/** A heartbeat rule's last check-in and when the next one is due. */
export interface HeartbeatState {
  lastCheckinAt: number | null;
  lastStatus: 'ok' | 'fail';
  lastMessage: string;
  dueAt: number;
  alarming: boolean;
  issuedAt: number;
}

export async function heartbeatState(id: string): Promise<HeartbeatState> {
  const { data } = await http.get<HeartbeatState>(`/alerts/rules/${encodeURIComponent(id)}/heartbeat`);
  return data;
}

/** A new check-in address; the old one stops working. */
export async function resetHeartbeatAddress(id: string): Promise<HeartbeatAddress> {
  const { data } = await http.post<HeartbeatAddress>(`/alerts/rules/${encodeURIComponent(id)}/heartbeat-token`);
  return data;
}
export async function remove(id: string) {
  await http.delete(`/alerts/rules/${id}`);
}

/** The rule bundle format the server exports and takes back. */
export const RULE_BUNDLE_FORMAT = 'molesignal.alert-rules/v1';

export interface RuleBundle {
  format: string;
  exportedAt?: string;
  rules: unknown[];
}

/** Every rule, or the ones named; their policies and child rules are named, not their ids. */
export async function exportRules(ids?: string[]): Promise<RuleBundle> {
  const { data } = await http.get<RuleBundle>('/alerts/rules/export', {
    params: ids && ids.length > 0 ? { ids: ids.join(',') } : {},
  });
  return data;
}

export type ImportAction = 'create' | 'update' | 'unchanged' | 'skip' | 'error';

/** One line of what an import has to say: `code` names the text, `params` fill it in. */
export interface ImportFinding {
  code: string;
  params?: Record<string, string>;
}

export interface ImportItem {
  name: string;
  action: ImportAction;
  /** The fields an update changes. */
  changes?: string[];
  error?: string;
  ruleId?: string;
  /** Emails with no member in the organization, every one of them. */
  unknownMembers?: string[];
  /** What cannot be carried over and would change the result; any of them stops the import. */
  blockers?: ImportFinding[];
  /** What was left out on purpose, and how the item compares with where it came from. */
  findings?: ImportFinding[];
}

export interface ImportReport {
  /** `molesignal` or `prometheus` for rules; the bundle format, `pagerduty` or `opsgenie` for the other kinds. */
  format: string;
  items: ImportItem[];
  /** What the file had that is not imported: recording rules, unsupported fields. */
  notes?: string[];
  /** What is said about the whole file. */
  findings?: ImportFinding[];
  applied: boolean;
}

/** A rule bundle, or a Prometheus rule file. Tried by default; `dry_run: false` writes it all at once. */
export async function importRules(input: {
  content: string;
  dryRun: boolean;
  createOnly: boolean;
}): Promise<ImportReport> {
  const { data } = await http.post<ImportReport>('/alerts/rules/import', input);
  return data;
}
