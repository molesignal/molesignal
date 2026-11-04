import type { AlertOutcome, AlertSource, AlertSourceKind, IntakeAlert, IntakeRequest } from '@/api/alertSources';
import { externalSourceOf } from '@/shell/incident/origin';
import type { Incident } from '@/types/alerting';

/** Stands for the token where it is not known: after creation it is never shown again. */
export const TOKEN_PLACEHOLDER = '<TOKEN>';

const TOKEN_IN_ADDRESS: readonly AlertSourceKind[] = ['aliyun_cms', 'tencent_monitor', 'aws_cloudwatch'];

/** A sender that cannot set a header: the token rides in the address. */
export function tokenInAddress(kind: AlertSourceKind): boolean {
  return TOKEN_IN_ADDRESS.includes(kind);
}

/** The address a source's alerts are sent to. A sender that cannot set headers carries the token in it. */
export function intakeAddress(origin: string, source: Pick<AlertSource, 'kind' | 'intakePath'>, token?: string): string {
  const url = `${origin.replace(/\/+$/, '')}${source.intakePath}`;

  if (!tokenInAddress(source.kind)) return url;
  return `${url}?token=${token ? encodeURIComponent(token) : TOKEN_PLACEHOLDER}`;
}

/** How the sender is set up, ready to paste: a configuration file, a request, or the fields to fill. */
export interface SetupSnippet {
  language: 'bash' | 'yaml' | 'json' | 'text';
  content: string;
}

/** The payload a Datadog webhook sends: our field names, Datadog's variables. */
export const DATADOG_PAYLOAD = `{
  "alert_id": "$ALERT_ID",
  "alert_cycle_key": "$ALERT_CYCLE_KEY",
  "transition": "$ALERT_TRANSITION",
  "alert_type": "$ALERT_TYPE",
  "priority": "$ALERT_PRIORITY",
  "title": "$ALERT_TITLE",
  "event_title": "$EVENT_TITLE",
  "message": "$TEXT_ONLY_MSG",
  "scope": "$ALERT_SCOPE",
  "tags": "$TAGS",
  "hostname": "$HOSTNAME",
  "link": "$LINK",
  "snapshot": "$SNAPSHOT",
  "query": "$ALERT_QUERY",
  "org_id": "$ORG_ID",
  "last_updated": "$LAST_UPDATED"
}`;

/** The PagerDuty events a webhook subscription sends for MoleSignal to follow. */
export const PAGERDUTY_EVENTS = [
  'incident.triggered',
  'incident.acknowledged',
  'incident.resolved',
  'incident.reopened',
  'incident.priority_updated',
];

/** The Zabbix media type to import: our field names, Zabbix's macros. The script posts them as they are. */
export function zabbixMediaType(url: string, token: string): string {
  const macros: Array<[string, string]> = [
    ['event_id', '{EVENT.ID}'],
    ['event_value', '{EVENT.VALUE}'],
    ['event_name', '{EVENT.NAME}'],
    ['event_time', '{EVENT.TIMESTAMP}'],
    ['recovery_time', '{EVENT.RECOVERY.TIMESTAMP}'],
    ['update_status', '{EVENT.UPDATE.STATUS}'],
    ['update_action', '{EVENT.UPDATE.ACTION}'],
    ['update_time', '{EVENT.UPDATE.TIMESTAMP}'],
    ['ack_status', '{EVENT.ACK.STATUS}'],
    ['severity', '{EVENT.NSEVERITY}'],
    ['host_name', '{HOST.NAME}'],
    ['host_ip', '{HOST.IP}'],
    ['trigger_id', '{TRIGGER.ID}'],
    ['operational_data', '{EVENT.OPDATA}'],
    ['tags', '{EVENT.TAGSJSON}'],
    ['subject', '{ALERT.SUBJECT}'],
    ['message', '{ALERT.MESSAGE}'],
    ['zabbix_url', '{$ZABBIX.URL}'],
  ];

  return `zabbix_export:
  version: '6.0'
  media_types:
    - name: MoleSignal
      type: WEBHOOK
      parameters:
        - name: url
          value: '${url}'
        - name: token
          value: '${token}'
${macros.map(([name, value]) => `        - name: ${name}\n          value: '${value}'`).join('\n')}
      script: |
        var params = JSON.parse(value);
        var url = params.url;
        var token = params.token;
        delete params.url;
        delete params.token;

        var request = new HttpRequest();
        request.addHeader('Content-Type: application/json');
        request.addHeader('Authorization: Bearer ' + token);

        var response = request.post(url, JSON.stringify(params));

        if (request.getStatus() !== 200) {
          throw 'MoleSignal answered ' + request.getStatus() + ': ' + response;
        }

        return 'OK';
      message_templates:
        - event_source: TRIGGERS
          operation_mode: PROBLEM
          subject: 'Problem: {EVENT.NAME}'
          message: 'Problem started at {EVENT.TIME} on {EVENT.DATE} on {HOST.NAME}: {EVENT.NAME}'
        - event_source: TRIGGERS
          operation_mode: RECOVERY
          subject: 'Resolved: {EVENT.NAME}'
          message: 'Resolved at {EVENT.RECOVERY.TIME} on {EVENT.RECOVERY.DATE}: {EVENT.NAME}'
        - event_source: TRIGGERS
          operation_mode: UPDATE
          subject: 'Updated: {EVENT.NAME}'
          message: '{USER.FULLNAME} {EVENT.UPDATE.ACTION} at {EVENT.UPDATE.TIME}: {EVENT.UPDATE.MESSAGE}'`;
}

/** A sender set up by its address alone has none: the address is all there is to fill in. */
export function setupSnippet(kind: AlertSourceKind, url: string, token: string = TOKEN_PLACEHOLDER): SetupSnippet | null {
  switch (kind) {
    case 'api':
      return {
        language: 'bash',
        content: `curl -X POST '${url}' \\
  -H 'Authorization: Bearer ${token}' \\
  -H 'Content-Type: application/json' \\
  -d '{"dedup_key": "disk-full/web-1", "summary": "Disk almost full on web-1", "severity": "critical", "labels": {"host.name": "web-1"}}'`,
      };
    case 'alertmanager':
      return {
        language: 'yaml',
        content: `receivers:
  - name: molesignal
    webhook_configs:
      - url: ${url}
        send_resolved: true
        http_config:
          authorization:
            credentials: ${token}`,
      };
    case 'grafana':
      return {
        language: 'yaml',
        content: `apiVersion: 1
contactPoints:
  - orgId: 1
    name: molesignal
    receivers:
      - uid: molesignal
        type: webhook
        settings:
          url: ${url}
          httpMethod: POST
          authorization_scheme: Bearer
          authorization_credentials: ${token}`,
      };
    case 'datadog':
      return {
        language: 'json',
        content: `URL: ${url}
Custom Headers: {"Authorization": "Bearer ${token}"}
Payload:
${DATADOG_PAYLOAD}`,
      };
    case 'pagerduty':
      return {
        language: 'text',
        content: `Webhook URL: ${url}
Event Subscription: ${PAGERDUTY_EVENTS.join(', ')}
Custom Header: Authorization = Bearer ${token}`,
      };
    case 'aliyun_cms':
    case 'tencent_monitor':
      // The token rides in the address.
      return null;
    case 'aws_cloudwatch':
      return {
        language: 'bash',
        content: `aws sns subscribe \\
  --topic-arn <SNS topic ARN of the alarm> \\
  --protocol https \\
  --notification-endpoint '${url}'`,
      };
    case 'zabbix':
      return { language: 'yaml', content: zabbixMediaType(url, token) };
    case 'email':
      // For a mail system that already takes the mail: the raw message is posted as it is.
      return {
        language: 'bash',
        content: `curl -X POST '${url}' \\
  -H 'Authorization: Bearer ${token}' \\
  -H 'Content-Type: message/rfc822' \\
  --data-binary @alert.eml`,
      };
  }
}

/** The outcomes of a request's alerts, the most consequential first, each with how many had it. */
export function outcomeCounts(alerts: readonly IntakeAlert[]): Array<{ outcome: AlertOutcome; count: number }> {
  const order: AlertOutcome[] = ['opened', 'updated', 'acknowledged', 'resolved', 'unchanged', 'ignored'];
  const counts = new Map<AlertOutcome, number>();

  for (const alert of alerts) counts.set(alert.outcome, (counts.get(alert.outcome) ?? 0) + 1);

  return order.filter((outcome) => counts.has(outcome)).map((outcome) => ({ outcome, count: counts.get(outcome) ?? 0 }));
}

/** The labels of a source written one per line, `key=value`, and read back. */
export function labelsToText(labels: Record<string, string>): string {
  return Object.entries(labels)
    .map(([key, value]) => `${key}=${value}`)
    .join('\n');
}

export function labelsFromPairs(pairs: ReadonlyArray<{ key: string; value: string }>): Record<string, string> {
  const labels: Record<string, string> = {};

  for (const { key, value } of pairs) {
    const name = key.trim();
    if (name) labels[name] = value.trim();
  }

  return labels;
}

const HOUR_MICROS = 3_600_000_000;
const DAY_HOURS = 24;

/**
 * How a source stands, from its latest request: off, never heard from, quiet
 * for longer than requests are kept, refused the last time, or receiving.
 */
export type SourceState = 'off' | 'waiting' | 'quiet' | 'refused' | 'receiving';

export function sourceState(
  source: Pick<AlertSource, 'enabled' | 'lastIntakeAt'>,
  requests: readonly IntakeRequest[],
): { state: SourceState; latest: IntakeRequest | undefined } {
  const latest = requests[0];

  if (!source.enabled) return { state: 'off', latest };
  if (!latest) return { state: source.lastIntakeAt ? 'quiet' : 'waiting', latest };
  return { state: latest.status === 'rejected' ? 'refused' : 'receiving', latest };
}

/** What a source received over the last day, from its latest requests (newest first). */
export interface DayFigures {
  requests: number;
  /** More were received than the page of requests holds: the count is a floor. */
  capped: boolean;
  refused: number;
  alerts: number;
  opened: number;
  resolved: number;
  /** Requests per hour, the oldest hour first. */
  hourly: number[];
}

export function dayFigures(requests: readonly IntakeRequest[], limit: number, nowMicros: number): DayFigures {
  const since = nowMicros - DAY_HOURS * HOUR_MICROS;
  const recent = requests.filter((request) => request.receivedAt >= since);
  const hourly = new Array<number>(DAY_HOURS).fill(0);
  const outcomes = recent.flatMap((request) => request.alerts.map((alert) => alert.outcome));

  for (const request of recent) {
    // A request stamped ahead of this clock counts in the latest hour.
    const age = Math.max(0, Math.floor((nowMicros - request.receivedAt) / HOUR_MICROS));
    const hour = DAY_HOURS - 1 - Math.min(age, DAY_HOURS - 1);
    hourly[hour] = (hourly[hour] ?? 0) + 1;
  }

  return {
    requests: recent.length,
    capped: requests.length >= limit && recent.length === requests.length,
    refused: recent.filter((request) => request.status === 'rejected').length,
    alerts: recent.reduce((sum, request) => sum + request.alertCount, 0),
    opened: outcomes.filter((outcome) => outcome === 'opened').length,
    resolved: outcomes.filter((outcome) => outcome === 'resolved').length,
    hourly,
  };
}

/** The incidents a source opened that are still open or acknowledged, the oldest first. */
export function activeIncidentsOf(sourceId: string, incidents: readonly Incident[]): Incident[] {
  return incidents
    .filter((incident) => externalSourceOf(incident)?.id === sourceId)
    .filter((incident) => incident.status === 'open' || incident.status === 'acknowledged')
    .sort((left, right) => left.createdAt - right.createdAt);
}

/** The labels of a group's alerts that tell them apart: those not every alert shares. */
export function distinctLabels(members: ReadonlyArray<{ labels: Record<string, string> }>): string[] {
  if (members.length < 2) return [];

  const keys = new Set(members.flatMap((member) => Object.keys(member.labels)));

  return [...keys]
    .filter((key) => new Set(members.map((member) => member.labels[key] ?? '')).size > 1)
    .sort();
}
