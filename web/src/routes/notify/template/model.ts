import type { TFunction } from 'i18next';

import type { NotifyCategory, NotifyChannel } from '@/api/notify';
import type { NotifyCard, NotifyCardColor } from '@/api/notify/templates';

import { resolveBlocks } from './blocks';
import { CONNECTOR_CHANNELS } from '../connector/channels';

export interface NotifyTemplatePreviewInput {
  eventId: string;
  eventType: string;
  occurredAt: number;
  attributes: Record<string, unknown>;
  messageTitle: string;
  messageText: string;
}

/** An alert that started five minutes before it was evaluated, as the server's test sends fill it. */
export const DEFAULT_NOTIFY_TEMPLATE_PREVIEW: NotifyTemplatePreviewInput = {
  eventId: 'alert.triggered:preview',
  eventType: 'alert.triggered',
  occurredAt: 1_785_283_200_000_000,
  attributes: {
    alert_id: 'incident-preview',
    rule_id: 'rule-preview',
    rule_name: 'API unavailable',
    rule: { description: 'The API answers fewer than 99% of requests.' },
    severity: 'critical',
    status: 'open',
    summary: 'API unavailable',
    fingerprint: 'api-unavailable',
    value: 512,
    threshold: 500,
    evaluated_at: 1_785_283_200_000_000,
    started_at: 1_785_282_900_000_000,
    incident_url: 'https://molesignal.example.com/alerts/incidents/incident-preview',
    service: 'api',
    environment: 'production',
    affected_services: ['api', 'checkout'],
    assignee_user_ids: ['alice'],
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
  occurredAt: 1_785_283_200_000_000,
  attributes: {
    schedule_id: 'schedule-preview',
    schedule_name: 'Primary',
    team_id: 'platform',
    timezone: 'Asia/Shanghai',
    current_user_id: 'alice',
    next_user_id: 'bob',
    transition_at: 1_785_285_000_000_000,
    override_id: 'override-preview',
    override_user_id: 'bob',
    original_user_id: 'alice',
    start_at: 1_785_283_200_000_000,
    end_at: 1_785_290_400_000_000,
    reason: 'Temporary coverage',
  },
  messageTitle: 'On-call shift starting soon',
  messageText: 'Primary changes on-call coverage.',
};

export const DEFAULT_REPORT_TEMPLATE_PREVIEW: NotifyTemplatePreviewInput = {
  eventId: 'report.ready:preview',
  eventType: 'report.ready',
  occurredAt: 1_785_283_200_000_000,
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
  occurredAt: 1_785_283_200_000_000,
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
  occurredAt: 1_785_283_200_000_000,
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
      attributes: {
        ...DEFAULT_NOTIFY_TEMPLATE_PREVIEW.attributes,
        step_index: 1,
        loop_index: 0,
        target_kind: 'schedule',
        escalation_reason: 'step',
      },
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
    occurredAt: 1_785_283_200_000_000,
    attributes: {},
    messageTitle: 'Notification preview',
    messageText: 'A notification event is ready for delivery.',
  };
}

/**
 * The preview of the event a preset is written for: the category's own, or the alert acknowledged
 * ten minutes after it was evaluated and resolved an hour after it started.
 */
export function notifyTemplatePreview(
  category: NotifyCategory,
  eventType?: string,
): NotifyTemplatePreviewInput {
  const preview = defaultNotifyTemplatePreview(category);
  if (category !== 'alert' || (eventType !== 'alert.acknowledged' && eventType !== 'alert.resolved')) {
    return preview;
  }

  const acknowledgedAt = 1_785_283_800_000_000;
  const resolvedAt = 1_785_286_800_000_000;
  const resolved = eventType === 'alert.resolved';
  const status = resolved ? 'resolved' : 'acknowledged';

  return {
    ...preview,
    eventId: `${eventType}:preview`,
    eventType,
    occurredAt: resolved ? resolvedAt : acknowledgedAt,
    attributes: {
      ...preview.attributes,
      status,
      acknowledged_by: 'alice',
      acknowledged_at: acknowledgedAt,
      ...(resolved ? { resolved_by: 'alice', resolved_at: resolvedAt } : {}),
    },
    messageText: `Alert API unavailable is ${status}.`,
  };
}

export function renderNotifyTemplate(
  body: string,
  input: NotifyTemplatePreviewInput,
): string {
  const values = templateValues(input);

  return resolveBlocks(body, values).replace(
    /{{\s*([^{}]+?)\s*}}/g,
    (placeholder, path: string) => values[path.trim()] ?? placeholder,
  );
}

/** Every value a template can name for an event, worked out as the server does. */
function templateValues(input: NotifyTemplatePreviewInput): Record<string, string> {
  const values: Record<string, string> = {
    'event.id': input.eventId,
    'event.type': input.eventType,
    'event.occurred_at': String(input.occurredAt),
    'message.title': input.messageTitle,
    'message.text': input.messageText,
  };

  for (const [path, value] of flattenAttributes(input.attributes)) {
    const rendered = scalar(value);
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
    'evaluated_at',
    'event.occurred_at',
  ]);
  alias(values, 'evaluated_at', [
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
  alias(values, 'oncall.transition_at', ['transition_at']);
  alias(values, 'override.id', ['override_id']);
  alias(values, 'override.user_id', ['override_user_id']);
  alias(values, 'override.original_user_id', ['original_user_id']);
  alias(values, 'override.start_at', ['start_at']);
  alias(values, 'override.end_at', ['end_at']);
  alias(values, 'override.reason', ['reason']);

  alias(values, 'incident.url', ['incident_url']);
  alias(values, 'incident.acknowledged_by', ['acknowledged_by']);
  alias(values, 'incident.resolved_by', ['resolved_by']);
  alias(values, 'incident.assignees', ['assignee_user_ids']);
  alias(values, 'escalation.target', ['target_kind']);
  alias(values, 'escalation.reason', ['escalation_reason']);

  // A text that takes replies gets its codes when it is sent; an alert preview shows a pair.
  if (input.eventType === 'alert.triggered' || input.eventType === 'alert.escalated') {
    values['reply.ack'] ??= '31';
    values['reply.resolve'] ??= '32';
  }

  addReadableValues(values, input);
  return values;
}

const READABLE_TIMES: ReadonlyArray<[string, string]> = [
  ['incident.started_time', 'started_at'],
  ['incident.acknowledged_time', 'acknowledged_at'],
  ['incident.resolved_time', 'resolved_at'],
  ['oncall.transition_time', 'transition_at'],
  ['override.start_time', 'start_at'],
  ['override.end_time', 'end_at'],
];

/** Values written for people: times in UTC, how long the alert lasted, every label, and the step and
 * round of an escalation counted from 1. `_at` is microseconds, `_time` the readable time. */
function addReadableValues(values: Record<string, string>, input: NotifyTemplatePreviewInput) {
  const { attributes } = input;
  const whole = (key: string) => {
    const value = attributes[key];
    return typeof value === 'number' && Number.isInteger(value) ? value : undefined;
  };

  values['event.time'] ??= readableTime(input.occurredAt);

  for (const [target, source] of READABLE_TIMES) {
    const at = whole(source);
    if (at !== undefined) values[target] ??= readableTime(at);
  }

  // An alert that is over counts until it was resolved, others until this event.
  const started = whole('started_at');
  if (started !== undefined) {
    values['incident.duration'] ??= durationText(
      (whole('resolved_at') ?? input.occurredAt) - started,
    );
  }

  const labels = attributes.labels;
  if (labels !== null && typeof labels === 'object' && !Array.isArray(labels)) {
    values.labels ??= Object.entries(labels as Record<string, unknown>)
      .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0))
      .map(([key, value]) => `${key}=${scalar(value)}`)
      .join(', ');
  }

  for (const [target, source] of [
    ['escalation.step', 'step_index'],
    ['escalation.loop', 'loop_index'],
  ] as const) {
    const index = whole(source);
    if (index !== undefined && index >= 0) values[target] ??= String(index + 1);
  }
}

/** Microseconds as people read them, in UTC: 2026-07-29 00:00:00 UTC. */
export function readableTime(micros: number): string {
  const date = new Date(Math.floor(micros / 1000));
  if (Number.isNaN(date.getTime())) return String(micros);
  const iso = date.toISOString();
  return `${iso.slice(0, 10)} ${iso.slice(11, 19)} UTC`;
}

const DURATION_UNITS: ReadonlyArray<[number, string]> = [
  [86_400, 'd'],
  [3_600, 'h'],
  [60, 'm'],
  [1, 's'],
];

/** A span written as its two largest units: 1h 5m, 4m 20s; under a second is 0s. */
export function durationText(micros: number): string {
  const seconds = Math.max(0, Math.trunc(micros / 1_000_000));
  const first = DURATION_UNITS.findIndex(([size]) => seconds >= size);
  if (first < 0) return '0s';

  const [size, unit] = DURATION_UNITS[first]!;
  let text = `${Math.floor(seconds / size)}${unit}`;
  const next = DURATION_UNITS[first + 1];
  if (next) {
    const rest = Math.floor((seconds % size) / next[0]);
    if (rest > 0) text += ` ${rest}${next[1]}`;
  }
  return text;
}

/** An attribute as a template writes it: text as it is, a list joined with ", ", the rest as JSON. */
function scalar(value: unknown): string {
  if (typeof value === 'string') return value;
  if (Array.isArray(value)) return value.map(scalar).join(', ');
  return JSON.stringify(value) ?? String(value);
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

/** A template's channel as the editor shows it: `general` is a template for every channel. */
export type TemplateChannel = 'general' | NotifyChannel;

/** General templates first, then the channels in the order the connector types list them. */
export const TEMPLATE_CHANNELS: readonly TemplateChannel[] = ['general', ...CONNECTOR_CHANNELS];

/** A channel's name as templates go by it; General is a template for every channel. */
export function templateChannelLabel(t: TFunction<'notify'>, channel: TemplateChannel): string {
  return t(`templates.channels.${channel}`);
}

/** SMS and phone templates are one plain text: their body is all that is sent. */
export function isPlainText(channel: TemplateChannel): channel is 'sms' | 'phone' {
  return channel === 'sms' || channel === 'phone';
}

/** The most an SMS sends and a call reads out; the server cuts what is longer. */
export const PLAIN_TEXT_LIMIT: Record<'sms' | 'phone', number> = { sms: 320, phone: 400 };

/** Characters as a phone counts them, not UTF-16 code units. */
export function characterCount(value: string): number {
  return [...value].length;
}

/** The colours a card's title takes, in the order the editor lists them. */
export const CARD_COLORS: readonly NotifyCardColor[] = [
  'severity',
  'blue',
  'green',
  'yellow',
  'orange',
  'red',
  'grey',
];

/** The most fields and links a card holds, as the server checks them. */
export const MAX_CARD_FIELDS = 10;
export const MAX_CARD_LINKS = 5;

/** A card to start from: what an alert or a shift change is about. A field or link an event cannot
 * fill is left out when it is sent. */
export function startingCard(category: NotifyCategory, t: TFunction<'notify'>): NotifyCard {
  const label = (key: string) => t(`templates.card.starting.${key}`);
  const alert = category === 'alert' || category === 'escalation';

  return {
    title: '',
    color: 'severity',
    fields: alert
      ? [
          { label: label('severity'), value: '{{severity}}' },
          { label: label('status'), value: '{{incident.status}}' },
          { label: label('service'), value: '{{labels.service}}' },
        ]
      : category === 'oncall'
        ? [
            { label: label('schedule'), value: '{{schedule.name}}' },
            { label: label('next'), value: '{{oncall.next_user_id}}' },
          ]
        : [],
    links: alert ? [{ label: label('runbook'), url: '{{annotations.runbook_url}}' }] : [],
    note: '',
  };
}

/** A card as it is saved: text trimmed, rows left empty dropped. */
export function cardToSave(card: NotifyCard): NotifyCard {
  return {
    title: card.title.trim(),
    color: card.color,
    fields: card.fields
      .map((field) => ({ label: field.label.trim(), value: field.value.trim() }))
      .filter((field) => field.label !== '' || field.value !== ''),
    links: card.links
      .map((link) => ({ label: link.label.trim(), url: link.url.trim() }))
      .filter((link) => link.label !== '' || link.url !== ''),
    note: card.note.trim(),
  };
}

export type CardProblem = 'too_many' | 'field' | 'link' | 'link_url' | 'too_long';

/** What keeps a card from being saved, checked as the server checks it. */
export function cardProblem(card: NotifyCard): CardProblem | null {
  const saved = cardToSave(card);
  const longer = (value: string, max: number) => characterCount(value) > max;

  if (saved.fields.length > MAX_CARD_FIELDS || saved.links.length > MAX_CARD_LINKS) return 'too_many';
  if (saved.fields.some((field) => field.label === '' || field.value === '')) return 'field';
  if (saved.links.some((link) => link.label === '' || link.url === '')) return 'link';
  if (saved.links.some((link) => !/^(https?:\/\/|\{\{)/.test(link.url))) return 'link_url';

  if (
    longer(saved.title, 200) ||
    longer(saved.note, 200) ||
    saved.fields.some((field) => longer(field.label, 40) || longer(field.value, 500)) ||
    saved.links.some((link) => longer(link.label, 30) || longer(link.url, 2000))
  ) {
    return 'too_long';
  }

  return null;
}

/** A card as an event fills it, the way the server does: a field or link with a placeholder the event
 * cannot fill is left out, and so is a link that is not an http(s) address. */
export function renderCard(card: NotifyCard, input: NotifyTemplatePreviewInput): NotifyCard {
  const filled = (value: string) => {
    const rendered = renderNotifyTemplate(value, input).trim();
    return /{{\s*[^{}]+?\s*}}/.test(rendered) ? null : rendered;
  };
  const saved = cardToSave(card);

  return {
    title: renderNotifyTemplate(saved.title, input).trim(),
    color: saved.color,
    fields: saved.fields.flatMap((field) => {
      const label = filled(field.label);
      const value = filled(field.value);
      return label && value ? [{ label, value }] : [];
    }),
    links: saved.links.flatMap((link) => {
      const label = filled(link.label);
      const url = filled(link.url);
      return label && url && /^https?:\/\//.test(url) ? [{ label, url }] : [];
    }),
    note: renderNotifyTemplate(saved.note, input).trim(),
  };
}

/** The colour a card's title takes for an event: by its severity, green once it is over. */
export function cardColor(
  color: NotifyCardColor,
  attributes: Record<string, unknown>,
): Exclude<NotifyCardColor, 'severity'> {
  if (color !== 'severity') return color;
  if (attributes.status === 'resolved' || attributes.status === 'closed') return 'green';
  if (attributes.severity === 'critical') return 'red';
  if (attributes.severity === 'error') return 'orange';
  if (attributes.severity === 'warning') return 'yellow';
  return 'blue';
}
