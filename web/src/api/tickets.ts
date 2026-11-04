import { http } from '@/lib/http';
import type { Severity } from '@/types/alerting';

import type { ConditionOperator } from './alertRouting';

export type TicketSystem = 'jira' | 'meegle' | 'servicenow';

export interface TicketAutoRule {
  enabled: boolean;
  conditions: Array<{ key: string; operator: ConditionOperator; values: string[] }>;
  minSeverity: Severity | null;
}

export interface TicketComments {
  acknowledged: boolean;
  resolved: boolean;
  notes: boolean;
}

/** Jira: Cloud signs in with an email and an API token, Data Center with a personal access token. */
export interface JiraSettings {
  siteUrl: string;
  deployment: 'cloud' | 'data_center';
  email: string;
  projectKey: string;
  issueType: string;
  /** Severity → priority name. */
  priorities: Record<string, string>;
  labels: string[];
  fields?: Record<string, unknown>;
  /** A transition's name or ID; empty takes the first one to a done status. */
  resolveTransition: string;
}

/** Feishu Project in China, Meegle elsewhere: the address follows the platform and is not edited. */
export type MeegleSite = 'feishu' | 'lark';

/** Meegle (Feishu Project): a plugin's ID and secret, acting as `user_key` in space `project_key`. */
export interface MeegleSettings {
  site: MeegleSite;
  pluginId: string;
  userKey: string;
  projectKey: string;
  workItemTypeKey: string;
  priorityField: string;
  priorities: Record<string, string>;
  fields?: Record<string, unknown>;
  resolveTransitionId: number | null;
  resolveNodeId: string;
  /** State keys that count as done. */
  doneStates: string[];
}

/** ServiceNow's Table API: a record in `table`, by basic authentication, OAuth client credentials or an API key. */
export interface ServiceNowSettings {
  instanceUrl: string;
  auth: 'basic' | 'oauth' | 'api_key';
  username: string;
  clientId: string;
  table: string;
  /** `sys_id`s. */
  assignmentGroup: string;
  callerId: string;
  category: string;
  /** Severity or priority → `impact/urgency`, or one number for both. */
  priorities: Record<string, string>;
  fields?: Record<string, unknown>;
  /** The `state` value a resolve writes. */
  resolveState: string;
  closeCode: string;
  /** `state` values that count as done. */
  doneStates: string[];
}

export interface TicketConnector {
  id: string;
  name: string;
  system: TicketSystem;
  settings: JiraSettings | MeegleSettings | ServiceNowSettings;
  /** Which secrets are set: `api_token`, `webhook_secret`, `plugin_secret`, `password`, `client_secret`, `api_key`. */
  secretsSet: string[];
  comments: TicketComments;
  transitionOnResolve: boolean;
  resolveIncidentWhenDone: boolean;
  auto: TicketAutoRule;
  onMajorIncident: boolean;
  enabled: boolean;
  /** Jira only: where Jira sends its webhook. */
  webhookPath?: string;
  createdAt: number;
  updatedAt: number;
}

export type TicketConnectorInput = Omit<TicketConnector, 'id' | 'secretsSet' | 'webhookPath' | 'createdAt' | 'updatedAt'> & {
  /** New secrets; an empty one keeps the saved value. */
  secrets: Record<string, string>;
};

/** A ticket made for an incident. */
export interface IncidentTicket {
  id: string;
  incidentId: string;
  connectorId: string;
  system: TicketSystem;
  externalId: string;
  externalKey: string;
  url: string;
  status: string;
  done: boolean;
  createdBy: string | null;
  createdAt: number;
  syncedAt: number;
  lastError: string;
}

export async function listConnectors(): Promise<TicketConnector[]> {
  const { data } = await http.get<{ connectors: TicketConnector[] }>('/alerts/tickets/connectors');
  return data.connectors;
}

/** The address of each Meegle platform, as the backend lists them. */
export async function listSites(): Promise<{ meegle: Array<{ key: MeegleSite; url: string }> }> {
  const { data } = await http.get<{ meegle: Array<{ key: MeegleSite; url: string }> }>('/alerts/tickets/sites');
  return data;
}

export async function createConnector(input: TicketConnectorInput): Promise<TicketConnector> {
  const { data } = await http.post<TicketConnector>('/alerts/tickets/connectors', input);
  return data;
}

export async function updateConnector(id: string, input: TicketConnectorInput): Promise<TicketConnector> {
  const { data } = await http.put<TicketConnector>(`/alerts/tickets/connectors/${encodeURIComponent(id)}`, input);
  return data;
}

export async function deleteConnector(id: string) {
  await http.delete(`/alerts/tickets/connectors/${encodeURIComponent(id)}`);
}

export async function testConnector(id: string): Promise<{ ok: boolean; detail: string }> {
  const { data } = await http.post<{ ok: boolean; detail: string }>(
    `/alerts/tickets/connectors/${encodeURIComponent(id)}/test`,
  );
  return data;
}

export async function incidentTickets(incidentId: string): Promise<IncidentTicket[]> {
  const { data } = await http.get<{ tickets: IncidentTicket[] }>(
    `/alerts/incidents/${encodeURIComponent(incidentId)}/tickets`,
  );
  return data.tickets;
}

export async function createTicket(incidentId: string, connectorId: string): Promise<IncidentTicket> {
  const { data } = await http.post<IncidentTicket>(
    `/alerts/incidents/${encodeURIComponent(incidentId)}/tickets`,
    { connectorId: connectorId },
  );
  return data;
}
