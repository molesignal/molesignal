import { http } from '@/lib/http';

import type { NotifyTargetType } from '../notify';

export type RiskLevel = 'l0' | 'l1' | 'l2' | 'l3' | 'l4';
export type ApprovalStatus =
  | 'pending'
  | 'approved'
  | 'rejected'
  | 'expired'
  | 'cancelled'
  | 'executed';
export type ExecutionStatus =
  | 'pending'
  | 'running'
  | 'succeeded'
  | 'partially_succeeded'
  | 'failed'
  | 'cancelled'
  | 'rolled_back'
  | 'verification_failed';

export interface ApprovalRequest {
  id: string;
  investigationId?: string | null;
  action: string;
  target: string;
  parameters: Record<string, unknown>;
  reason: string;
  impact: string;
  risk: RiskLevel;
  status: ApprovalStatus;
  requestedBy: string;
  requiredApprovals: number;
  reviews: Array<Record<string, unknown>>;
  expiresAt?: number | null;
  decidedAt?: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface Execution {
  id: string;
  approvalRequestId: string;
  investigationId?: string | null;
  action: string;
  target: string;
  parameters: Record<string, unknown>;
  idempotencyKey: string;
  requestedBy: string;
  approvedBy: string[];
  status: ExecutionStatus;
  outputSummary?: string | null;
  error?: string | null;
  verification: Record<string, unknown>;
  oneTimeResult?: {
    apiToken?: {
      id: string;
      name: string;
      prefix: string;
      token: string;
      roleId: string;
      roleKey: string;
      roleName: string;
      tokenKind: 'personal' | 'service_account';
      serviceAccountId?: string | null;
      expiresAt: number;
      createdAt: number;
    };
  };
  startedAt?: number | null;
  finishedAt?: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface ApprovalReviewResult {
  approval: ApprovalRequest;
  execution?: Execution | null;
}

export async function listApprovals(): Promise<ApprovalRequest[]> {
  const { data } = await http.get<{ approvals: ApprovalRequest[] }>('/agent/approvals');
  return data.approvals ?? [];
}

export async function reviewApproval(
  id: string,
  approve: boolean,
  comment: string,
): Promise<ApprovalReviewResult> {
  const { data } = await http.post<ApprovalReviewResult>(
    `/agent/approvals/${encodeURIComponent(id)}/review`,
    { approve, comment },
  );
  return data;
}

export async function executeApproval(id: string, idempotencyKey: string): Promise<Execution> {
  const { data } = await http.post<Execution>(
    `/agent/approvals/${encodeURIComponent(id)}/execute`,
    { idempotencyKey: idempotencyKey },
  );
  return data;
}

export async function listExecutions(): Promise<Execution[]> {
  const { data } = await http.get<{ executions: Execution[] }>('/agent/executions');
  return data.executions ?? [];
}

/**
 * Automations: Mole Agent doing a job with nobody in the chat. A rule is a
 * name, a scope, a task and a trigger; each time it triggers, the task is
 * asked in a hidden chat that the people who see the rule can open, as the
 * person who last saved it. Its outputs are notices on channels and, for
 * incidents, the incident's RCA.
 */
export type AutomationScope =
  | { scope: 'org' }
  | { scope: 'team'; teamId: string }
  | { scope: 'personal'; ownerId: string };

export interface IncidentFilter {
  severities: string[];
  ruleIds: string[];
  services: string[];
  labels: Record<string, string>;
}

/** Which new findings of automatic discovery start it; each field left empty takes all. */
export interface FindingFilter {
  kinds: string[];
  minSeverity?: string | null;
  subjects: string[];
}

export type AutomationTrigger =
  | { kind: 'schedule'; cron: string; timezone: string }
  | ({ kind: 'incident' } & IncidentFilter)
  | ({ kind: 'finding' } & FindingFilter)
  | { kind: 'api' }
  | { kind: 'manual' };

export type AutomationTriggerKind = AutomationTrigger['kind'];

/** A channel the result goes to: a fixed address or group on a notify connector. */
export interface AutomationChannel {
  connectorId: string;
  targetType: NotifyTargetType;
  target: string;
}

export interface AutomationOutputs {
  channels: AutomationChannel[];
  /** An automation started by incidents writes its conclusion as the incident's RCA. */
  writeRca: boolean;
}

export type AutomationRunStatus =
  | 'running'
  | 'succeeded'
  | 'partial'
  | 'failed'
  | 'skipped'
  | 'blocked'
  | 'abandoned';

export interface Automation {
  id: string;
  scope: AutomationScope;
  name: string;
  description: string;
  prompt: string;
  enabled: boolean;
  trigger: AutomationTrigger;
  outputs: AutomationOutputs;
  /** Who it runs as: whoever saved it last. */
  runAsUserId: string;
  language: string;
  timeoutSecs: number;
  template?: string | null;
  /** The start of its API token; the token itself is shown once. */
  apiTokenPrefix?: string | null;
  nextRunAt?: number | null;
  lastRun?: { id: string; at: number; status: AutomationRunStatus } | null;
  createdBy: string;
  updatedBy: string;
  createdAt: number;
  updatedAt: number;
  /** The reader can change it, run it and manage its token. */
  editable: boolean;
  /** The reader can disable or delete it. */
  moderatable: boolean;
}

export interface AutomationInput {
  name: string;
  description: string;
  prompt: string;
  enabled: boolean;
  trigger: AutomationTrigger;
  outputs: AutomationOutputs;
  timeoutSecs: number;
  /** New ones only: whose it is, and the template it started from. */
  scope?: 'org' | 'team' | 'personal';
  teamId?: string;
  template?: string | null;
}

export interface AutomationRun {
  id: string;
  automationId: string;
  trigger: AutomationTriggerKind;
  status: AutomationRunStatus;
  /** Codes, comma separated: `budget_exhausted`, `credential`, `delivery`… */
  reason?: string | null;
  chatId?: string | null;
  runId?: string | null;
  incidentId?: string | null;
  /** What the run started from: the caller's text, or the incident's summary. */
  input?: string | null;
  /** The start of the answer. */
  summary?: string | null;
  outputs: {
    deliveries?: Array<{
      connectorId: string;
      targetType: string;
      targetMasked: string;
      delivered: boolean;
      error?: string | null;
    }>;
    rca?: boolean | null;
  };
  startedBy?: string | null;
  createdAt: number;
  finishedAt?: number | null;
}

export interface AutomationTemplate {
  key: string;
  name: string;
  description: string;
  prompt: string;
  trigger: AutomationTrigger;
  outputs: AutomationOutputs;
}

export interface AutomationToken {
  token: string;
  prefix: string;
  /** Where to send it: `POST <instance>/api/v1/public/automations/<id>/trigger`. */
  triggerPath: string;
}

const automationPath = (id: string) => `/agent/automations/${encodeURIComponent(id)}`;

export async function listAutomations(): Promise<Automation[]> {
  const { data } = await http.get<{ automations: Automation[] }>('/agent/automations');
  return data.automations ?? [];
}

/** The automations the reader sees, and whether they can create one. */
export async function listAutomationsPage(): Promise<{
  automations: Automation[];
  canCreate: boolean;
}> {
  const { data } = await http.get<{ automations: Automation[]; canCreate: boolean }>(
    '/agent/automations',
  );
  return { automations: data.automations ?? [], canCreate: data.canCreate ?? false };
}

export async function createAutomation(input: AutomationInput): Promise<Automation> {
  const { data } = await http.post<Automation>('/agent/automations', input);
  return data;
}

export async function updateAutomation(id: string, input: AutomationInput): Promise<Automation> {
  const { data } = await http.put<Automation>(automationPath(id), input);
  return data;
}

export async function deleteAutomation(id: string): Promise<boolean> {
  const { data } = await http.delete<{ deleted: boolean }>(automationPath(id));
  return data.deleted;
}

export async function setAutomationEnabled(id: string, enabled: boolean): Promise<Automation> {
  const { data } = await http.post<Automation>(`${automationPath(id)}/enabled`, { enabled });
  return data;
}

/** Run it now, as the person it runs as; once a minute at most. */
export async function runAutomation(id: string): Promise<AutomationRun> {
  const { data } = await http.post<{ run: AutomationRun }>(`${automationPath(id)}/run`);
  return data.run;
}

export async function rotateAutomationToken(id: string): Promise<AutomationToken> {
  const { data } = await http.post<AutomationToken>(`${automationPath(id)}/token`);
  return data;
}

export async function revokeAutomationToken(id: string): Promise<void> {
  await http.delete(`${automationPath(id)}/token`);
}

export async function listAutomationRuns(id: string, limit = 50): Promise<AutomationRun[]> {
  const { data } = await http.get<{ runs: AutomationRun[] }>(`${automationPath(id)}/runs`, {
    params: { limit },
  });
  return data.runs ?? [];
}

/** Built-in templates in the reader's language; scheduled ones in `timezone`. */
export async function listAutomationTemplates(timezone: string): Promise<AutomationTemplate[]> {
  const { data } = await http.get<{ templates: AutomationTemplate[] }>(
    '/agent/automation-templates',
    { params: { timezone } },
  );
  return data.templates ?? [];
}
