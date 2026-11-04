import { http } from '@/lib/http';
import type { TimeCondition, Urgency } from '@/types/alerting';

// Notify management API surface.
export type NotifyCategory =
  | 'alert'
  | 'oncall'
  | 'escalation'
  | 'report'
  | 'security'
  | 'system';
export type NotifyDeliveryMode =
  | 'prefer_user'
  | 'force_connector'
  | 'multi_connector';
/** The channel a connector delivers through. Templates are written for one, or for all. */
export type NotifyChannel = 'im' | 'phone' | 'sms' | 'email' | 'webhook';
export type NotifyTargetType =
  | 'direct_user'
  | 'fixed_address'
  | 'fixed_group'
  | 'webhook';
export type NotifyDeliveryStage =
  | 'user_primary'
  | 'user_fallback'
  | 'team_fallback'
  | 'organization_fallback'
  | 'escalation'
  | 'test'
  /** Sent straight to a channel a feature chose itself, outside any policy. */
  | 'direct';
export type NotifyDeliveryStatus =
  | 'pending'
  | 'sending'
  | 'success'
  | 'failed'
  | 'skipped'
  | 'acknowledged'
  /** Waiting on a phone line: the number was just called, and this joins the next, merged call. */
  | 'queued';

export interface ConnectorCapabilities {
  directUser: boolean;
  group: boolean;
  richText: boolean;
  interactive: boolean;
  acknowledgement: boolean;
  attachments: boolean;
}

export interface NotifyConnector {
  id: string;
  organizationId: string;
  name: string;
  connectorType: string;
  config: Record<string, unknown>;
  capabilities: ConnectorCapabilities;
  enabled: boolean;
  status: 'unknown' | 'connected' | 'error';
  lastTestedAt?: number | null;
  lastTestStatus?: 'success' | 'failed' | null;
  lastTestError?: string | null;
  createdAt: number;
  updatedAt: number;
}

/** Connector types that send only templates their provider approved, with the provider's name: the
 * template code and its variables are written into SMS and phone templates under that name. Such a
 * connector delivers through notify policies only, never straight from a feature. */
const TEMPLATE_PROVIDERS: Readonly<Record<string, string>> = {
  aliyun_sms: 'aliyun',
  aliyun_voice: 'aliyun',
  tencent_sms: 'tencent',
  tencent_voice: 'tencent',
  huawei_sms: 'huawei',
  huawei_voice: 'huawei',
};

export function templateProvider(connectorType: string): string | null {
  return TEMPLATE_PROVIDERS[connectorType] ?? null;
}

/** IM connector types, apps and group bots alike, that send an instant messaging template's card as
 * a card of their own; without a card they send text. */
const CARD_CONNECTOR_TYPES: ReadonlySet<string> = new Set([
  'lark_app',
  'lark_webhook',
  'dingtalk_app',
  'dingtalk_webhook',
  'wecom_app',
  'wecom_webhook',
  'slack_app',
  'slack_webhook',
  'teams_app',
  'teams_webhook',
  'google_chat_app',
  'google_chat_webhook',
]);

export function sendsCards(connectorType: string): boolean {
  return CARD_CONNECTOR_TYPES.has(connectorType);
}

export interface NotifyConnectorType {
  connectorType: string;
  capabilities: ConnectorCapabilities;
  /** The provider's addresses the type's settings keep; the backend saves no others. */
  addresses?: NotifyConnectorAddress[];
}

/** A setting that holds a provider's address: it takes one of `choices`, the first the default, and is
 * shown, not edited. */
export interface NotifyConnectorAddress {
  /** The setting, such as `apiBaseUrl`, `serviceUrl` or `wsUrl`. */
  field: string;
  /** What one of several addresses is picked by (`site`, `cloud`); the choice itself is not saved. */
  picker?: string;
  choices: Array<{ key: string; url: string }>;
  /** The address follows the chosen region, as the provider lists it; `choices` holds the one used
   * before a region is chosen, if any. */
  byRegion?: { service: string; domains: string[] };
}

/** A region a regional connector (Alibaba Cloud, AWS) can use, as its provider lists it, with the
 * address requests go to in it. */
export interface NotifyConnectorRegion {
  id: string;
  name: string;
  apiBaseUrl: string;
}

export interface NotifyConnectorInput {
  name: string;
  connectorType: string;
  config: Record<string, unknown>;
  enabled: boolean;
}

export interface NotifyConnectorUpdate {
  name: string;
  config?: Record<string, unknown>;
  enabled: boolean;
}

export interface NotifyTestResult {
  sent: boolean;
  testedAt: number;
  elapsedMs: number;
  providerMessageId?: string | null;
  error?: string | null;
}

export interface UserNotifyEndpoint {
  id: string;
  organizationId: string;
  userId: string;
  connectorId: string;
  providerType: string;
  externalIdentity: string;
  displayName?: string | null;
  metadata: Record<string, unknown>;
  verified: boolean;
  enabled: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface UserNotifyEndpointInput {
  connectorId: string;
  externalIdentity: string;
  displayName?: string | null;
  metadata?: Record<string, unknown>;
  verified?: boolean;
  enabled: boolean;
}

export interface UserNotifyPreferenceStep {
  id: string;
  preferenceId: string;
  endpointId: string;
  stepOrder: number;
  /** Seconds after the person was first notified that this step is used if nobody acknowledged. */
  delaySecs: number;
  /** Which of the two step lists it is in; only alert and escalation have a low one. */
  urgency?: Urgency;
  createdAt: number;
}

export interface UserNotifyPreference {
  id: string;
  organizationId: string;
  userId: string;
  category: NotifyCategory;
  enabled: boolean;
  quietHours?: Record<string, unknown> | null;
  /** High-urgency alerts still come during quiet hours; low-urgency ones never do. */
  allowHighUrgencyBypass: boolean;
  steps: UserNotifyPreferenceStep[];
  createdAt: number;
  updatedAt: number;
}

export interface UserNotifyPreferenceInput {
  enabled: boolean;
  endpointIds: string[];
  /** One per endpoint, in the same order; left out, no step waits. */
  stepDelaysSecs?: number[];
  /** The low-urgency steps (alert and escalation only); left empty, low urgency uses the first step above. */
  lowEndpointIds?: string[];
  lowStepDelaysSecs?: number[];
  quietHours?: Record<string, unknown> | null;
  allowHighUrgencyBypass: boolean;
}

export interface NotifyUserSummary {
  userId: string;
  email: string;
  displayName: string;
  avatarUrl?: string | null;
  disabled: boolean;
  status: 'active' | 'pending' | 'rejected';
  endpoints: UserNotifyEndpoint[];
  preferences: UserNotifyPreference[];
}

export interface NotifyFallbackConfig {
  useUserFallbacks: boolean;
  useTeamDefaults: boolean;
  useOrganizationDefaults: boolean;
}

export interface NotifyDeliveryConfig {
  connectorIds: string[];
}

export interface NotifyPolicyInput {
  name: string;
  eventType: string;
  category: NotifyCategory;
  matchers: Record<string, unknown>;
  recipientResolver: string;
  resolverConfig: Record<string, unknown>;
  deliveryMode: NotifyDeliveryMode;
  deliveryConfig: NotifyDeliveryConfig;
  /** The general template: used by every channel the policy chose none for. */
  templateId?: string | null;
  /** A template of their own for some channels, each one of that channel. */
  channelTemplates?: Partial<Record<NotifyChannel, string>>;
  fallbackConfig: NotifyFallbackConfig;
  ackTimeoutSeconds?: number | null;
  escalationConfig?: Record<string, unknown> | null;
  /** Alert notifications to one recipient and channel within this many seconds go out as one. */
  mergeWindowSecs?: number | null;
  /** Only inside (or outside) these hours, by the time the notification's event happened. */
  time?: TimeCondition | null;
  enabled: boolean;
  priority: number;
}

export interface NotifyPolicy extends NotifyPolicyInput {
  id: string;
  organizationId: string;
  /** The built-in policy this one was copied from when the organization was created: it keeps its
   * name, is turned off rather than deleted, and can be restored. A policy of the organization's
   * own has none. */
  builtinKey: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface NotifyPreviewEvent {
  eventId?: string;
  eventType?: string;
  occurredAt?: number;
  attributes: Record<string, unknown>;
}

export interface NotifyDeliveryPlanStep {
  stage: NotifyDeliveryStage;
  connectorId: string;
  connectorName: string;
  endpointId?: string | null;
  targetType: NotifyTargetType;
  targetValueMasked: string;
  /** The template this step is sent with; none sends the event's own message. */
  templateId?: string | null;
}

export interface NotifyRecipientPlan {
  userId: string;
  teamId?: string | null;
  resolvedBy: string;
  deliveryPlan: NotifyDeliveryPlanStep[];
}

export interface NotifyPolicyPreview {
  policyId: string;
  matched: boolean;
  recipients: NotifyRecipientPlan[];
}

export interface NotifyDelivery {
  id: string;
  organizationId: string;
  eventId: string;
  policyId?: string | null;
  recipientUserId?: string | null;
  connectorId?: string | null;
  endpointId?: string | null;
  targetType: string;
  targetValueMasked?: string | null;
  stage: NotifyDeliveryStage;
  attempt: number;
  status: NotifyDeliveryStatus;
  errorCode?: string | null;
  errorMessage?: string | null;
  latencyMs?: number | null;
  sentAt?: number | null;
  deliveredAt?: number | null;
  acknowledgedAt?: number | null;
  escalatedAt?: number | null;
  idempotencyKey: string;
  createdAt: number;
}

export interface NotifyDefaultRoute {
  connectorId: string;
  targetType: NotifyTargetType;
  target: string;
  order: number;
}

export interface NotifyDefault {
  id: string;
  organizationId: string;
  teamId?: string;
  category: NotifyCategory;
  routes: NotifyDefaultRoute[];
  enabled: boolean;
  createdAt: number;
  updatedAt: number;
}

export async function listConnectorTypes(): Promise<NotifyConnectorType[]> {
  const { data } = await http.get<NotifyConnectorType[]>('/notify/connector-types');
  return data;
}

/** The external ID this organization's AWS connectors assume roles with. */
export async function awsExternalId(): Promise<string> {
  const { data } = await http.get<{ externalId: string }>('/notify/aws-external-id');
  return data.externalId;
}

/** The regions a connector type can use, asked of its provider with the settings being filled in.
 * `connector_id` lets the masked secrets of a saved connector stand for the stored ones. */
export async function listConnectorRegions(input: {
  connectorType: string;
  config: Record<string, unknown>;
  connectorId?: string;
  language: string;
}): Promise<NotifyConnectorRegion[]> {
  const { data } = await http.post<{ regions?: NotifyConnectorRegion[] }>('/notify/connector-regions', input);
  return data.regions ?? [];
}

export async function listConnectors(): Promise<NotifyConnector[]> {
  const { data } = await http.get<NotifyConnector[]>('/notify/connectors');
  return data;
}

export async function createConnector(input: NotifyConnectorInput): Promise<NotifyConnector> {
  const { data } = await http.post<NotifyConnector>('/notify/connectors', input);
  return data;
}

export async function updateConnector(
  id: string,
  input: NotifyConnectorUpdate,
): Promise<NotifyConnector> {
  const { data } = await http.put<NotifyConnector>(`/notify/connectors/${id}`, input);
  return data;
}

export async function removeConnector(id: string): Promise<void> {
  await http.delete(`/notify/connectors/${id}`);
}

/** Where a connector's provider sends replies and keypresses. An address is `null` when the type has
 * none to set (a Twilio call carries its own) or the server has no external URL. */
export interface NotifyConnectorCallbacks {
  externalUrlSet: boolean;
  repliesUrl: string | null;
  keypressUrl: string | null;
  /** Where a WeCom app's Receive messages go: the presses of its alert card buttons. */
  messageUrl: string | null;
}

export async function connectorCallbacks(id: string): Promise<NotifyConnectorCallbacks> {
  const { data } = await http.get<NotifyConnectorCallbacks>(`/notify/connectors/${id}/callbacks`);
  return data;
}

/** Sends a test through a connector; with a template, the template filled with sample values. */
export async function testConnector(
  id: string,
  targetType: NotifyTargetType,
  target: string,
  templateId?: string,
): Promise<NotifyTestResult> {
  const { data } = await http.post<NotifyTestResult>(`/notify/connectors/${id}/test`, {
    targetType: targetType,
    target,
    ...(templateId ? { templateId } : {}),
  });
  return data;
}

export async function listNotifyUsers(): Promise<NotifyUserSummary[]> {
  const { data } = await http.get<NotifyUserSummary[]>('/notify/users');
  return data;
}

export async function listEndpoints(userId: string): Promise<UserNotifyEndpoint[]> {
  const { data } = await http.get<UserNotifyEndpoint[]>(
    `/users/${userId}/notify-endpoints`,
  );
  return data;
}

export async function createEndpoint(
  userId: string,
  input: UserNotifyEndpointInput,
): Promise<UserNotifyEndpoint> {
  const { data } = await http.post<UserNotifyEndpoint>(
    `/users/${userId}/notify-endpoints`,
    input,
  );
  return data;
}

export async function updateEndpoint(
  userId: string,
  id: string,
  input: UserNotifyEndpointInput,
): Promise<UserNotifyEndpoint> {
  const { data } = await http.put<UserNotifyEndpoint>(
    `/users/${userId}/notify-endpoints/${id}`,
    input,
  );
  return data;
}

export async function removeEndpoint(userId: string, id: string): Promise<void> {
  await http.delete(`/users/${userId}/notify-endpoints/${id}`);
}

export async function verifyEndpoint(userId: string, id: string): Promise<UserNotifyEndpoint> {
  const { data } = await http.post<UserNotifyEndpoint>(
    `/users/${userId}/notify-endpoints/${id}/verify`,
  );
  return data;
}

export async function testEndpoint(userId: string, id: string): Promise<NotifyTestResult> {
  const { data } = await http.post<NotifyTestResult>(
    `/users/${userId}/notify-endpoints/${id}/test`,
    {},
  );
  return data;
}

export async function listPreferences(userId: string): Promise<UserNotifyPreference[]> {
  const { data } = await http.get<UserNotifyPreference[]>(
    `/users/${userId}/notify-preferences`,
  );
  return data;
}

export async function updatePreference(
  userId: string,
  category: NotifyCategory,
  input: UserNotifyPreferenceInput,
): Promise<UserNotifyPreference> {
  const { data } = await http.put<UserNotifyPreference>(
    `/users/${userId}/notify-preferences/${category}`,
    input,
  );
  return data;
}

export async function listResolverTypes(): Promise<string[]> {
  const { data } = await http.get<string[]>('/notify/recipient-resolver-types');
  return data;
}

export async function listPolicies(): Promise<NotifyPolicy[]> {
  const { data } = await http.get<NotifyPolicy[]>('/notify/policies');
  return data;
}

export async function createPolicy(input: NotifyPolicyInput): Promise<NotifyPolicy> {
  const { data } = await http.post<NotifyPolicy>('/notify/policies', input);
  return data;
}

export async function updatePolicy(
  id: string,
  input: NotifyPolicyInput,
): Promise<NotifyPolicy> {
  const { data } = await http.put<NotifyPolicy>(`/notify/policies/${id}`, input);
  return data;
}

export async function removePolicy(id: string): Promise<void> {
  await http.delete(`/notify/policies/${id}`);
}

/** Puts a built-in policy back as it is built in. */
export async function restorePolicy(id: string): Promise<NotifyPolicy> {
  const { data } = await http.post<NotifyPolicy>(`/notify/policies/${id}/restore`);
  return data;
}

export async function previewPolicy(
  policy: NotifyPolicyInput,
  event: NotifyPreviewEvent,
): Promise<NotifyPolicyPreview> {
  const { data } = await http.post<NotifyPolicyPreview>('/notify/policies/preview', {
    policy,
    event,
  });
  return data;
}

export async function listDeliveries(
  filters: Record<string, string | number | undefined>,
): Promise<NotifyDelivery[]> {
  const { data } = await http.get<NotifyDelivery[]>('/notify/deliveries', {
    params: filters,
  });
  return data;
}

export async function acknowledgeDelivery(id: string): Promise<NotifyDelivery> {
  const { data } = await http.post<NotifyDelivery>(`/notify/deliveries/${id}/ack`);
  return data;
}

export async function retryDelivery(id: string): Promise<void> {
  await http.post(`/notify/deliveries/${id}/retry`);
}

export async function listOrganizationDefaults(): Promise<NotifyDefault[]> {
  const { data } = await http.get<NotifyDefault[]>('/notify/organization-defaults');
  return data;
}

export async function updateOrganizationDefault(
  category: NotifyCategory,
  routes: NotifyDefaultRoute[],
  enabled: boolean,
): Promise<NotifyDefault> {
  const { data } = await http.put<NotifyDefault>(
    `/notify/organization-defaults/${category}`,
    { routes, enabled },
  );
  return data;
}

export async function removeOrganizationDefault(
  category: NotifyCategory,
): Promise<void> {
  await http.delete(`/notify/organization-defaults/${category}`);
}

export async function listTeamDefaults(teamId: string): Promise<NotifyDefault[]> {
  const { data } = await http.get<NotifyDefault[]>(
    `/notify/team-defaults/${encodeURIComponent(teamId)}`,
  );
  return data;
}

export async function updateTeamDefault(
  teamId: string,
  category: NotifyCategory,
  routes: NotifyDefaultRoute[],
  enabled: boolean,
): Promise<NotifyDefault> {
  const { data } = await http.put<NotifyDefault>(
    `/notify/team-defaults/${encodeURIComponent(teamId)}/${category}`,
    { routes, enabled },
  );
  return data;
}

export async function removeTeamDefault(
  teamId: string,
  category: NotifyCategory,
): Promise<void> {
  await http.delete(
    `/notify/team-defaults/${encodeURIComponent(teamId)}/${category}`,
  );
}

/** Whether someone can get alerts: their own methods, only through a team or organization group, or not at all. */
export type Reachability = 'personal' | 'group_only' | 'none';

/** Whether these people can get alerts (`notify.policies.read`); answers only the state, never the methods. */
export async function reachability(userIds: string[]): Promise<Record<string, Reachability>> {
  if (userIds.length === 0) return {};
  const { data } = await http.get<{ users: { userId: string; status: Reachability }[] }>(
    '/notify/reachability',
    { params: { userIds: userIds.join(',') } },
  );
  return Object.fromEntries(data.users.map((user) => [user.userId, user.status]));
}
