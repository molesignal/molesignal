import { http } from '@/lib/http';

import type {
  ActiveAutomationRule,
  AutomationApprovalInput,
  AutomationCandidate,
  AutomationCandidateDetail,
  AutomationCandidateState,
  AutomationRule,
  AutomationRuleInput,
  AutomationRuleLifecycle,
  AutomationRevision,
  AutomationSimulation,
  AutomationSimulationInput,
  StatusPage,
  StatusPageAccessRule,
  StatusPageAccessRuleKind,
  StatusPageAccessSession,
  StatusPageComponent,
  StatusPageComponentInput,
  StatusPageDeliveryPage,
  StatusPageDomainCapability,
  StatusPageDomainInstructions,
  StatusPageEventList,
  StatusPageEventView,
  StatusPageHistoryPage,
  StatusPageIncident,
  StatusPageIncidentInput,
  StatusPageIncidentKind,
  StatusPageIncidentUpdateInput,
  StatusPageInput,
  StatusPageLifecycle,
  StatusPageSnapshot,
  StatusPageSubscriber,
  StatusPageSubscriberPage,
} from './types';

const pagePath = (pageId: string) => `/status-pages/${encodeURIComponent(pageId)}`;

export async function list(lifecycle: StatusPageLifecycle = 'active'): Promise<StatusPage[]> {
  const { data } = await http.get<StatusPage[]>('/status-pages', { params: { lifecycle } });
  return data;
}

export async function get(pageId: string): Promise<StatusPageSnapshot> {
  const { data } = await http.get<StatusPageSnapshot>(pagePath(pageId));
  return data;
}

export async function create(input: StatusPageInput): Promise<StatusPage> {
  const { data } = await http.post<StatusPage>('/status-pages', input);
  return data;
}

export async function update(pageId: string, input: StatusPageInput): Promise<StatusPage> {
  const { data } = await http.put<StatusPage>(pagePath(pageId), input);
  return data;
}

export async function archive(pageId: string): Promise<StatusPage> {
  const { data } = await http.post<StatusPage>(`${pagePath(pageId)}/archive`);
  return data;
}

export async function restore(pageId: string): Promise<StatusPage> {
  const { data } = await http.post<StatusPage>(`${pagePath(pageId)}/restore`);
  return data;
}

export async function remove(pageId: string): Promise<void> {
  await http.delete(pagePath(pageId));
}

export async function uploadLogo(pageId: string, file: File): Promise<StatusPage> {
  const { data } = await http.post<StatusPage>(`${pagePath(pageId)}/logo`, file, {
    headers: { 'Content-Type': file.type || 'application/octet-stream' },
  });
  return data;
}

export async function getLogoBlob(pageId: string): Promise<Blob> {
  const { data } = await http.get<Blob>(`${pagePath(pageId)}/logo`, {
    responseType: 'blob',
  });
  return data;
}

export async function createComponent(
  pageId: string,
  input: StatusPageComponentInput,
): Promise<StatusPageComponent> {
  const { data } = await http.post<StatusPageComponent>(`${pagePath(pageId)}/components`, input);
  return data;
}

export async function updateComponent(
  pageId: string,
  componentId: string,
  input: StatusPageComponentInput,
): Promise<StatusPageComponent> {
  const { data } = await http.put<StatusPageComponent>(
    `${pagePath(pageId)}/components/${encodeURIComponent(componentId)}`,
    input,
  );
  return data;
}

export async function removeComponent(pageId: string, componentId: string): Promise<void> {
  await http.delete(`${pagePath(pageId)}/components/${encodeURIComponent(componentId)}`);
}

export async function listEvents(
  pageId: string,
  kind: StatusPageIncidentKind,
  view: StatusPageEventView,
): Promise<StatusPageEventList> {
  const { data } = await http.get<StatusPageEventList>(`${pagePath(pageId)}/incidents`, {
    params: { kind, view },
  });
  return data;
}

export async function getEvent(pageId: string, eventId: string): Promise<StatusPageIncident> {
  const { data } = await http.get<StatusPageIncident>(
    `${pagePath(pageId)}/incidents/${encodeURIComponent(eventId)}`,
  );
  return data;
}

export async function createIncident(
  pageId: string,
  input: StatusPageIncidentInput,
): Promise<StatusPageIncident> {
  const { data } = await http.post<StatusPageIncident>(`${pagePath(pageId)}/incidents`, input);
  return data;
}

export async function updateEvent(
  pageId: string,
  eventId: string,
  input: StatusPageIncidentInput,
): Promise<StatusPageIncident> {
  const { data } = await http.put<StatusPageIncident>(
    `${pagePath(pageId)}/incidents/${encodeURIComponent(eventId)}`,
    input,
  );
  return data;
}

export async function publishDraft(
  pageId: string,
  eventId: string,
  input: StatusPageIncidentInput,
): Promise<StatusPageIncident> {
  const { data } = await http.post<StatusPageIncident>(
    `${pagePath(pageId)}/incidents/${encodeURIComponent(eventId)}/publish`,
    input,
  );
  return data;
}

export async function appendIncidentUpdate(
  pageId: string,
  eventId: string,
  input: StatusPageIncidentUpdateInput,
): Promise<StatusPageIncident> {
  const { data } = await http.post<StatusPageIncident>(
    `${pagePath(pageId)}/incidents/${encodeURIComponent(eventId)}/updates`,
    input,
  );
  return data;
}

export interface HistoryFilters {
  type?: StatusPageIncidentKind;
  status?: string;
  component?: string;
  from?: number;
  to?: number;
  q?: string;
  page?: number;
}

export async function history(
  pageId: string,
  filters: HistoryFilters,
): Promise<StatusPageHistoryPage> {
  const { data } = await http.get<StatusPageHistoryPage>(`${pagePath(pageId)}/history`, {
    params: filters,
  });
  return data;
}

export async function listSubscribers(
  pageId: string,
  page: number,
): Promise<StatusPageSubscriberPage> {
  const { data } = await http.get<StatusPageSubscriberPage>(`${pagePath(pageId)}/subscribers`, {
    params: { page },
  });
  return data;
}

export async function revokeSubscriber(
  pageId: string,
  subscriberId: string,
): Promise<StatusPageSubscriber> {
  const { data } = await http.delete<StatusPageSubscriber>(
    `${pagePath(pageId)}/subscribers/${encodeURIComponent(subscriberId)}`,
  );
  return data;
}

export async function resendSubscriber(
  pageId: string,
  subscriberId: string,
): Promise<StatusPageSubscriber> {
  const { data } = await http.post<StatusPageSubscriber>(
    `${pagePath(pageId)}/subscribers/${encodeURIComponent(subscriberId)}/resend`,
  );
  return data;
}

export async function listDeliveries(
  pageId: string,
  page: number,
): Promise<StatusPageDeliveryPage> {
  const { data } = await http.get<StatusPageDeliveryPage>(`${pagePath(pageId)}/deliveries`, {
    params: { page },
  });
  return data;
}

export async function getDomain(pageId: string): Promise<StatusPageDomainInstructions | null> {
  const { data } = await http.get<StatusPageDomainInstructions | null>(`${pagePath(pageId)}/domain`);
  return data;
}

export async function getDomainCapability(pageId: string): Promise<StatusPageDomainCapability> {
  const { data } = await http.get<StatusPageDomainCapability>(
    `${pagePath(pageId)}/domain/capability`,
  );
  return data;
}

export async function configureDomain(
  pageId: string,
  hostname: string,
): Promise<StatusPageDomainInstructions> {
  const { data } = await http.put<StatusPageDomainInstructions>(`${pagePath(pageId)}/domain`, {
    hostname,
  });
  return data;
}

export async function verifyDomain(pageId: string): Promise<StatusPageDomainInstructions> {
  const { data } = await http.post<StatusPageDomainInstructions>(`${pagePath(pageId)}/domain/verify`);
  return data;
}

export async function retryDomain(pageId: string): Promise<StatusPageDomainInstructions> {
  const { data } = await http.post<StatusPageDomainInstructions>(`${pagePath(pageId)}/domain/retry`);
  return data;
}

export async function removeDomain(pageId: string): Promise<void> {
  await http.delete(`${pagePath(pageId)}/domain`);
}

export async function listAccessRules(pageId: string): Promise<StatusPageAccessRule[]> {
  const { data } = await http.get<StatusPageAccessRule[]>(`${pagePath(pageId)}/access-rules`);
  return data;
}

export async function createAccessRule(
  pageId: string,
  kind: StatusPageAccessRuleKind,
  value: string,
): Promise<StatusPageAccessRule> {
  const { data } = await http.post<StatusPageAccessRule>(`${pagePath(pageId)}/access-rules`, {
    kind,
    value,
  });
  return data;
}

export async function removeAccessRule(pageId: string, ruleId: string): Promise<void> {
  await http.delete(`${pagePath(pageId)}/access-rules/${encodeURIComponent(ruleId)}`);
}

export async function listAccessSessions(pageId: string): Promise<StatusPageAccessSession[]> {
  const { data } = await http.get<StatusPageAccessSession[]>(`${pagePath(pageId)}/access-sessions`);
  return data;
}

export async function revokeAccessSession(pageId: string, sessionId: string): Promise<void> {
  await http.delete(`${pagePath(pageId)}/access-sessions/${encodeURIComponent(sessionId)}`);
}

export async function revokeAllAccessSessions(pageId: string): Promise<number> {
  const { data } = await http.delete<{ revoked: number }>(`${pagePath(pageId)}/access-sessions`);
  return data.revoked;
}

export async function listAutomationRules(pageId: string): Promise<ActiveAutomationRule[]> {
  const { data } = await http.get<ActiveAutomationRule[]>(`${pagePath(pageId)}/automation/rules`);
  return data;
}

export async function createAutomationRule(
  pageId: string,
  input: AutomationRuleInput,
): Promise<ActiveAutomationRule> {
  const { data } = await http.post<ActiveAutomationRule>(
    `${pagePath(pageId)}/automation/rules`,
    input,
  );
  return data;
}

export async function reorderAutomationRules(
  pageId: string,
  ruleIds: string[],
): Promise<ActiveAutomationRule[]> {
  const { data } = await http.put<ActiveAutomationRule[]>(
    `${pagePath(pageId)}/automation/rules/order`,
    { rule_ids: ruleIds },
  );
  return data;
}

export async function createAutomationRevision(
  pageId: string,
  ruleId: string,
  input: AutomationRuleInput,
): Promise<AutomationRevision> {
  const { data } = await http.post<AutomationRevision>(
    `${pagePath(pageId)}/automation/rules/${encodeURIComponent(ruleId)}/revisions`,
    input,
  );
  return data;
}

export async function activateAutomationRevision(
  pageId: string,
  ruleId: string,
  revisionId: string,
): Promise<ActiveAutomationRule> {
  const { data } = await http.post<ActiveAutomationRule>(
    `${pagePath(pageId)}/automation/rules/${encodeURIComponent(ruleId)}/revisions/${encodeURIComponent(revisionId)}/activate`,
  );
  return data;
}

export async function setAutomationRuleLifecycle(
  pageId: string,
  ruleId: string,
  lifecycle: AutomationRuleLifecycle,
): Promise<AutomationRule> {
  const { data } = await http.put<AutomationRule>(
    `${pagePath(pageId)}/automation/rules/${encodeURIComponent(ruleId)}/lifecycle`,
    { lifecycle },
  );
  return data;
}

export async function simulateAutomation(
  pageId: string,
  input: AutomationSimulationInput,
): Promise<AutomationSimulation> {
  const { data } = await http.post<AutomationSimulation>(
    `${pagePath(pageId)}/automation/simulate`,
    input,
  );
  return data;
}

export async function listAutomationCandidates(
  pageId: string,
  state?: AutomationCandidateState,
): Promise<AutomationCandidate[]> {
  const { data } = await http.get<AutomationCandidate[]>(
    `${pagePath(pageId)}/automation/candidates`,
    { params: { state, limit: 100 } },
  );
  return data;
}

export async function listPendingAutomationCandidates(): Promise<AutomationCandidate[]> {
  const { data } = await http.get<AutomationCandidate[]>(
    '/status-pages/automation/candidates/pending',
  );
  return data;
}

export async function getAutomationCandidateDetail(
  pageId: string,
  candidateId: string,
): Promise<AutomationCandidateDetail> {
  const { data } = await http.get<AutomationCandidateDetail>(
    `${pagePath(pageId)}/automation/candidates/${encodeURIComponent(candidateId)}`,
  );
  return data;
}

export async function approveAutomationCandidate(
  pageId: string,
  candidateId: string,
  input: AutomationApprovalInput,
): Promise<AutomationCandidate> {
  const { data } = await http.post<AutomationCandidate>(
    `${pagePath(pageId)}/automation/candidates/${encodeURIComponent(candidateId)}/approve`,
    input,
  );
  return data;
}

export async function rejectAutomationCandidate(
  pageId: string,
  candidateId: string,
  note?: string,
): Promise<AutomationCandidate> {
  const { data } = await http.post<AutomationCandidate>(
    `${pagePath(pageId)}/automation/candidates/${encodeURIComponent(candidateId)}/reject`,
    { note },
  );
  return data;
}

export async function retryAutomationCandidate(
  pageId: string,
  candidateId: string,
): Promise<AutomationCandidate> {
  const { data } = await http.post<AutomationCandidate>(
    `${pagePath(pageId)}/automation/candidates/${encodeURIComponent(candidateId)}/retry`,
  );
  return data;
}

export async function getAutomationSettings(pageId: string): Promise<{ paused: boolean }> {
  const { data } = await http.get<{ paused: boolean }>(`${pagePath(pageId)}/automation/settings`);
  return data;
}

export async function setAutomationSettings(
  pageId: string,
  paused: boolean,
): Promise<{ paused: boolean }> {
  const { data } = await http.put<{ paused: boolean }>(
    `${pagePath(pageId)}/automation/settings`,
    { paused },
  );
  return data;
}
