import type { NotifyCategory } from '@/api/notify';

export interface NotifyTemplatePreviewInput {
  eventId: string;
  eventType: string;
  occurredAtMicros: number;
  attributes: Record<string, unknown>;
  messageTitle: string;
  messageText: string;
}

export const DEFAULT_NOTIFY_TEMPLATE_PREVIEW: NotifyTemplatePreviewInput = {
  eventId: 'alert.triggered:preview',
  eventType: 'alert.triggered',
  occurredAtMicros: 1_785_283_200_000_000,
  attributes: {
    alert_id: 'incident-preview',
    rule_id: 'rule-preview',
    rule_name: 'API unavailable',
    severity: 'critical',
    status: 'open',
    summary: 'API unavailable',
    fingerprint: 'api-unavailable',
    value: 512,
    threshold: 500,
    evaluated_at_micros: 1_785_283_200_000_000,
    labels: {
      service: 'api',
      env: 'production',
      environment: 'production',
    },
    annotations: {
      runbook_url: 'https://runbooks.example.com/api-unavailable',
    },
  },
  messageTitle: 'CRITICAL · API unavailable',
  messageText: 'Alert API unavailable is open.',
};

export const DEFAULT_ONCALL_TEMPLATE_PREVIEW: NotifyTemplatePreviewInput = {
  eventId: 'oncall.shift.starting:schedule-preview',
  eventType: 'oncall.shift.starting',
  occurredAtMicros: 1_785_283_200_000_000,
  attributes: {
    schedule_id: 'schedule-preview',
    schedule_name: 'Primary',
    team_id: 'platform',
    timezone: 'Asia/Shanghai',
    current_user_id: 'alice',
    next_user_id: 'bob',
    transition_at_micros: 1_785_285_000_000_000,
    override_id: 'override-preview',
    override_user_id: 'bob',
    original_user_id: 'alice',
    start_at_micros: 1_785_283_200_000_000,
    end_at_micros: 1_785_290_400_000_000,
    reason: 'Temporary coverage',
  },
  messageTitle: 'On-call shift starting soon',
  messageText: 'Primary changes on-call coverage.',
};

export const DEFAULT_REPORT_TEMPLATE_PREVIEW: NotifyTemplatePreviewInput = {
  eventId: 'report.ready:preview',
  eventType: 'report.ready',
  occurredAtMicros: 1_785_283_200_000_000,
  attributes: {
    report_id: 'report-preview',
    report_name: 'Weekly reliability report',
    period_start: '2026-07-20',
    period_end: '2026-07-26',
    download_url: 'https://reports.example.com/weekly-reliability',
  },
  messageTitle: 'Weekly reliability report is ready',
  messageText: 'The scheduled report was generated successfully.',
};

export const DEFAULT_SECURITY_TEMPLATE_PREVIEW: NotifyTemplatePreviewInput = {
  eventId: 'security.access.detected:preview',
  eventType: 'security.access.detected',
  occurredAtMicros: 1_785_283_200_000_000,
  attributes: {
    action: 'Delete API token',
    actor: 'alice@example.com',
    resource: 'api-token/production',
    ip_address: '203.0.113.42',
  },
  messageTitle: 'SECURITY · Suspicious access detected',
  messageText: 'A privileged action was observed from a new IP address.',
};

export const DEFAULT_SYSTEM_TEMPLATE_PREVIEW: NotifyTemplatePreviewInput = {
  eventId: 'system.health.changed:preview',
  eventType: 'system.health.changed',
  occurredAtMicros: 1_785_283_200_000_000,
  attributes: {
    component: 'query-gateway',
    status: 'degraded',
    region: 'cn-south-1',
  },
  messageTitle: 'System component is degraded',
  messageText: 'Query latency is above the expected operating range.',
};

export function defaultNotifyTemplatePreview(
  category: NotifyCategory,
): NotifyTemplatePreviewInput {
  if (category === 'alert') return DEFAULT_NOTIFY_TEMPLATE_PREVIEW;
  if (category === 'escalation') {
    return {
      ...DEFAULT_NOTIFY_TEMPLATE_PREVIEW,
      eventId: 'alert.escalated:preview',
      eventType: 'alert.escalated',
      messageTitle: 'ESCALATION · API unavailable',
      messageText: 'Alert API unavailable requires escalation.',
    };
  }
  if (category === 'oncall') return DEFAULT_ONCALL_TEMPLATE_PREVIEW;
  if (category === 'report') return DEFAULT_REPORT_TEMPLATE_PREVIEW;
  if (category === 'security') return DEFAULT_SECURITY_TEMPLATE_PREVIEW;
  if (category === 'system') return DEFAULT_SYSTEM_TEMPLATE_PREVIEW;
  return {
    eventId: `${category}.preview`,
    eventType: `${category}.preview`,
    occurredAtMicros: 1_785_283_200_000_000,
    attributes: {},
    messageTitle: 'Notification preview',
    messageText: 'A notification event is ready for delivery.',
  };
}

export function renderNotifyTemplate(
  body: string,
  input: NotifyTemplatePreviewInput,
): string {
  const values: Record<string, string> = {
    'event.id': input.eventId,
    'event.type': input.eventType,
    'event.occurred_at': String(input.occurredAtMicros),
    'message.title': input.messageTitle,
    'message.text': input.messageText,
  };

  for (const [path, value] of flattenAttributes(input.attributes)) {
    const rendered =
      typeof value === 'string' ? value : JSON.stringify(value) ?? String(value);
    values[path] = rendered;
    values[`event.attributes.${path}`] = rendered;
  }

  alias(values, 'occurred_at', ['event.occurred_at']);
  alias(values, 'rule.id', ['rule_id']);
  alias(values, 'rule.name', ['rule_name', 'summary', 'incident.summary']);
  alias(values, 'rule.description', ['rule_description']);
  values['rule.description'] ??= '';
  alias(values, 'incident.id', ['incident_id', 'alert_id']);
  alias(values, 'incident.fingerprint', ['fingerprint']);
  alias(values, 'incident.status', ['status']);
  alias(values, 'incident.summary', ['summary']);
  alias(values, 'evaluated_at', [
    'evaluated_at_micros',
    'event.occurred_at',
  ]);
  alias(values, 'evaluated_at_micros', [
    'evaluated_at',
    'event.occurred_at',
  ]);
  alias(values, 'rule_name', ['rule.name', 'summary']);
  alias(values, 'incident_id', ['incident.id', 'alert_id']);
  values.value ??= 'null';
  values.threshold ??= 'null';

  alias(values, 'schedule.id', ['schedule_id']);
  alias(values, 'schedule.name', ['schedule_name']);
  alias(values, 'schedule.team_id', ['team_id']);
  alias(values, 'schedule.timezone', ['timezone']);
  alias(values, 'oncall.current_user_id', ['current_user_id']);
  alias(values, 'oncall.next_user_id', ['next_user_id']);
  alias(values, 'oncall.transition_at', ['transition_at_micros']);
  alias(values, 'override.id', ['override_id']);
  alias(values, 'override.user_id', ['override_user_id']);
  alias(values, 'override.original_user_id', ['original_user_id']);
  alias(values, 'override.start_at', ['start_at_micros']);
  alias(values, 'override.end_at', ['end_at_micros']);
  alias(values, 'override.reason', ['reason']);

  return body.replace(
    /{{\s*([^{}]+?)\s*}}/g,
    (placeholder, path: string) => values[path.trim()] ?? placeholder,
  );
}

function alias(
  values: Record<string, string>,
  target: string,
  sources: string[],
) {
  if (values[target] !== undefined) return;
  const source = sources.find((candidate) => values[candidate] !== undefined);
  if (source) values[target] = values[source]!;
}

function flattenAttributes(
  value: Record<string, unknown>,
  prefix = '',
): Array<[string, unknown]> {
  const result: Array<[string, unknown]> = [];
  for (const [key, child] of Object.entries(value)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (
      child !== null &&
      !Array.isArray(child) &&
      typeof child === 'object'
    ) {
      result.push(
        ...flattenAttributes(child as Record<string, unknown>, path),
      );
    } else {
      result.push([path, child]);
    }
  }
  return result;
}
