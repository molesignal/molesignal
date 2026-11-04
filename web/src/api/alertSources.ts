import { http } from '@/lib/http';
import type { Severity } from '@/types/alerting';

/** Which sender's format a source reads. */
export type AlertSourceKind =
  | 'api'
  | 'alertmanager'
  | 'grafana'
  | 'datadog'
  | 'pagerduty'
  | 'aliyun_cms'
  | 'tencent_monitor'
  | 'aws_cloudwatch'
  | 'zabbix'
  | 'email';

export const ALERT_SOURCE_KINDS: readonly AlertSourceKind[] = [
  'api',
  'alertmanager',
  'grafana',
  'datadog',
  'pagerduty',
  'aliyun_cms',
  'tencent_monitor',
  'aws_cloudwatch',
  'zabbix',
  'email',
];

/** How an email source reads a mail. */
export interface EmailRules {
  /** Addresses, `@example.com` for a whole domain, or `*` for everyone. */
  allowedSenders: string[];
  /** Subjects that recover the alert; empty takes the built-in one. */
  resolvePattern: string;
  /** Subjects that are not alerts. */
  ignorePattern: string;
  /** Takes the dedup key from the subject; empty uses the tidied subject. */
  dedupPattern: string;
}

/** The IMAP mailbox an email source reads, without its secret. */
export interface MailboxSettings {
  host: string;
  port: number;
  security: 'tls' | 'starttls' | 'none';
  username: string;
  auth: { kind: 'password' } | { kind: 'oauth2'; tokenUrl: string; clientId: string; scope: string };
  folder: string;
  pollSecs: number;
}

export interface Mailbox {
  settings: MailboxSettings;
  secretSet: boolean;
  /** It has connected once, and reads from there on. */
  reading: boolean;
  lastPolledAt: number | null;
  lastSuccessAt: number | null;
  lastError: string;
  updatedAt: number;
}

/** An outside system that sends alerts in. The token itself is only in the answer that made it. */
export interface AlertSource {
  id: string;
  name: string;
  kind: AlertSourceKind;
  enabled: boolean;
  /** `default`, a policy id, or `null` to page nobody and only notify. */
  escalationPolicyId: string | null;
  /** Used when an alert carries no severity of its own. */
  severity: Severity;
  labels: Record<string, string>;
  /** `mspat-` and the 10 characters a token is found by. */
  tokenPrefix: string;
  /** The federation cluster its incidents open on; empty for this cluster. */
  homeCluster: string;
  /** The path every source shares: `/api/v1/intake/alerts`. */
  intakePath: string;
  createdBy: string | null;
  createdAt: number;
  updatedAt: number;
  lastIntakeAt: number | null;
  /** Email sources only: how a mail is read. */
  email?: EmailRules;
}

/** A source as it is answered right after its token was made: the only time the token is seen. */
export interface IssuedAlertSource extends AlertSource {
  token: string;
}

export interface AlertSourceInput {
  kind?: AlertSourceKind;
  name: string;
  enabled: boolean;
  escalationPolicyId: string | null;
  severity: Severity;
  labels: Record<string, string>;
  homeCluster?: string;
  email?: EmailRules;
}

/** A federation cluster a source's incidents can open on. */
export interface ClusterChoice {
  id: string;
  name: string;
  local: boolean;
}

export type IntakeStatus = 'accepted' | 'rejected' | 'forwarded';

export type AlertOutcome = 'opened' | 'updated' | 'unchanged' | 'acknowledged' | 'resolved' | 'ignored';

/** One alert of a request, and what became of it. */
export interface IntakeAlert {
  action: 'trigger' | 'acknowledge' | 'resolve';
  dedupKey: string;
  summary: string;
  severity: Severity | null;
  outcome: AlertOutcome;
  reason?: string;
  incidentId?: string;
}

/** A request a source received. */
export interface IntakeRequest {
  id: string;
  receivedAt: number;
  /** The federation cluster that passed it on; empty when it came here directly. */
  viaCluster: string;
  status: IntakeStatus;
  httpStatus: number;
  message: string | null;
  alertCount: number;
  alerts: IntakeAlert[];
  /** The start of a refused body. */
  bodyExcerpt: string | null;
}

/** One alert of a sender's notification group, as last reported. */
export interface AlertMember {
  status: 'firing' | 'resolved';
  labels: Record<string, string>;
  summary?: string;
  startsAt?: number;
  endsAt?: number;
  url?: string;
}

/** What an alert source knows of an incident it opened. */
export interface IncidentSource {
  /** Null once the source was deleted. */
  source: AlertSource | null;
  dedupKey: string;
  lastEventAt: number | null;
  lastIntakeAt: number;
  members: AlertMember[];
}

const base = '/alerts/sources';
const path = (id: string) => `${base}/${encodeURIComponent(id)}`;

export async function list(): Promise<AlertSource[]> {
  const { data } = await http.get<AlertSource[]>(base);
  return data;
}

export async function get(id: string): Promise<AlertSource> {
  const { data } = await http.get<AlertSource>(path(id));
  return data;
}

export async function create(input: AlertSourceInput): Promise<IssuedAlertSource> {
  const { data } = await http.post<IssuedAlertSource>(base, input);
  return data;
}

export async function update(id: string, input: AlertSourceInput): Promise<AlertSource> {
  const { data } = await http.put<AlertSource>(path(id), input);
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(path(id));
}

export async function rotateToken(id: string): Promise<IssuedAlertSource> {
  const { data } = await http.post<IssuedAlertSource>(`${path(id)}/token`);
  return data;
}

export async function requests(id: string, limit = 50): Promise<IntakeRequest[]> {
  const { data } = await http.get<IntakeRequest[]>(`${path(id)}/requests`, { params: { limit } });
  return data;
}

export async function mailbox(id: string): Promise<Mailbox | null> {
  const { data } = await http.get<Mailbox | null>(`/alerts/sources/${encodeURIComponent(id)}/mailbox`);
  return data;
}

export type MailboxInput = MailboxSettings & { secret: string };

export async function saveMailbox(id: string, input: MailboxInput): Promise<Mailbox> {
  const { data } = await http.put<Mailbox>(`/alerts/sources/${encodeURIComponent(id)}/mailbox`, input);
  return data;
}

export async function removeMailbox(id: string): Promise<void> {
  await http.delete(`/alerts/sources/${encodeURIComponent(id)}/mailbox`);
}

export async function testMailbox(id: string, input: MailboxInput): Promise<{ ok: boolean; messages: number }> {
  const { data } = await http.post<{ ok: boolean; messages: number }>(
    `/alerts/sources/${encodeURIComponent(id)}/mailbox/test`,
    input,
  );
  return data;
}

export async function clusters(): Promise<{ local: string; clusters: ClusterChoice[] }> {
  const { data } = await http.get<{ local: string; clusters: ClusterChoice[] }>(`${base}/clusters`);
  return data;
}

export async function incidentSource(incidentId: string): Promise<IncidentSource> {
  const { data } = await http.get<IncidentSource>(
    `/alerts/incidents/${encodeURIComponent(incidentId)}/source`,
  );
  return data;
}
