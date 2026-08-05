import { http } from '@/lib/http';

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
  | 'test';
export type NotifyDeliveryStatus =
  | 'pending'
  | 'sending'
  | 'success'
  | 'failed'
  | 'skipped'
  | 'acknowledged';

export interface ConnectorCapabilities {
  direct_user: boolean;
  group: boolean;
  rich_text: boolean;
  interactive: boolean;
  acknowledgement: boolean;
  attachments: boolean;
}

export interface NotifyConnector {
  id: string;
  organization_id: string;
  name: string;
  connector_type: string;
  config: Record<string, unknown>;
  capabilities: ConnectorCapabilities;
  enabled: boolean;
  status: 'unknown' | 'connected' | 'error';
  last_tested_at?: number | null;
  last_test_status?: 'success' | 'failed' | null;
  last_test_error?: string | null;
  created_at: number;
  updated_at: number;
}

export interface NotifyConnectorType {
  connector_type: string;
  capabilities: ConnectorCapabilities;
}

export interface NotifyConnectorInput {
  name: string;
  connector_type: string;
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
  tested_at_micros: number;
  elapsed_ms: number;
  provider_message_id?: string | null;
  error?: string | null;
}

export interface UserNotifyEndpoint {
  id: string;
  organization_id: string;
  user_id: string;
  connector_id: string;
  provider_type: string;
  external_identity: string;
  display_name?: string | null;
  metadata: Record<string, unknown>;
  verified: boolean;
  enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface UserNotifyEndpointInput {
  connector_id: string;
  external_identity: string;
  display_name?: string | null;
  metadata?: Record<string, unknown>;
  verified?: boolean;
  enabled: boolean;
}

export interface UserNotifyPreferenceStep {
  id: string;
  preference_id: string;
  endpoint_id: string;
  step_order: number;
  created_at: number;
}

export interface UserNotifyPreference {
  id: string;
  organization_id: string;
  user_id: string;
  category: NotifyCategory;
  enabled: boolean;
  quiet_hours?: Record<string, unknown> | null;
  allow_critical_bypass: boolean;
  steps: UserNotifyPreferenceStep[];
  created_at: number;
  updated_at: number;
}

export interface UserNotifyPreferenceInput {
  enabled: boolean;
  endpoint_ids: string[];
  quiet_hours?: Record<string, unknown> | null;
  allow_critical_bypass: boolean;
}

export interface NotifyUserSummary {
  user_id: string;
  email: string;
  display_name: string;
  avatar_url?: string | null;
  disabled: boolean;
  status: 'active' | 'pending' | 'rejected';
  endpoints: UserNotifyEndpoint[];
  preferences: UserNotifyPreference[];
}

export interface NotifyFallbackConfig {
  use_user_fallbacks: boolean;
  use_team_defaults: boolean;
  use_organization_defaults: boolean;
}

export interface NotifyDeliveryConfig {
  connector_ids: string[];
}

export interface NotifyPolicyInput {
  name: string;
  event_type: string;
  category: NotifyCategory;
  matchers: Record<string, unknown>;
  recipient_resolver: string;
  resolver_config: Record<string, unknown>;
  delivery_mode: NotifyDeliveryMode;
  delivery_config: NotifyDeliveryConfig;
  template_id?: string | null;
  fallback_config: NotifyFallbackConfig;
  ack_timeout_seconds?: number | null;
  escalation_config?: Record<string, unknown> | null;
  enabled: boolean;
  priority: number;
}

export interface NotifyPolicy extends NotifyPolicyInput {
  id: string;
  organization_id: string;
  created_at: number;
  updated_at: number;
}

export interface NotifyPreviewEvent {
  event_id?: string;
  event_type?: string;
  occurred_at_micros?: number;
  attributes: Record<string, unknown>;
}

export interface NotifyDeliveryPlanStep {
  stage: NotifyDeliveryStage;
  connector_id: string;
  connector_name: string;
  endpoint_id?: string | null;
  target_type: NotifyTargetType;
  target_value_masked: string;
}

export interface NotifyRecipientPlan {
  user_id: string;
  team_id?: string | null;
  resolved_by: string;
  delivery_plan: NotifyDeliveryPlanStep[];
}

export interface NotifyPolicyPreview {
  policy_id: string;
  matched: boolean;
  recipients: NotifyRecipientPlan[];
}

export interface NotifyDelivery {
  id: string;
  organization_id: string;
  event_id: string;
  policy_id?: string | null;
  recipient_user_id?: string | null;
  connector_id?: string | null;
  endpoint_id?: string | null;
  target_type: string;
  target_value_masked?: string | null;
  stage: NotifyDeliveryStage;
  attempt: number;
  status: NotifyDeliveryStatus;
  error_code?: string | null;
  error_message?: string | null;
  latency_ms?: number | null;
  sent_at?: number | null;
  delivered_at?: number | null;
  acknowledged_at?: number | null;
  escalated_at?: number | null;
  idempotency_key: string;
  created_at: number;
}

export interface NotifyDefaultRoute {
  connector_id: string;
  target_type: NotifyTargetType;
  target: string;
  order: number;
}

export interface NotifyDefault {
  id: string;
  organization_id: string;
  team_id?: string;
  category: NotifyCategory;
  routes: NotifyDefaultRoute[];
  enabled: boolean;
  created_at: number;
  updated_at: number;
}

export async function listConnectorTypes(): Promise<NotifyConnectorType[]> {
  const { data } = await http.get<NotifyConnectorType[]>('/notify/connector-types');
  return data;
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

export async function testConnector(
  id: string,
  targetType: NotifyTargetType,
  target: string,
): Promise<NotifyTestResult> {
  const { data } = await http.post<NotifyTestResult>(`/notify/connectors/${id}/test`, {
    target_type: targetType,
    target,
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
