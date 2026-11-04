import type { TFunction } from 'i18next';

import type {
  NotifyConnector,
  NotifyDeliveryStage,
  NotifyDeliveryStatus,
  NotifyPolicy,
  NotifyTargetType,
  UserNotifyEndpoint,
  UserNotifyPreference,
} from '@/api/notify';

export const NOTIFY_CATEGORIES = [
  'alert',
  'oncall',
  'report',
  'escalation',
  'security',
  'system',
] as const;

export const EVENT_TYPES = [
  'alert.triggered',
  'alert.acknowledged',
  'alert.resolved',
  'alert.escalated',
  'incident.status_update',
  'oncall.shift.starting',
  'oncall.shift.started',
  'oncall.override.created',
  'oncall.coverage.missing',
  'oncall.swap.updated',
  'oncall.member.unreachable',
  'intake.budget.warning',
  'intake.budget.exceeded',
] as const;

/** A policy's name; a built-in one in the reader's language, of which its saved name is the English. */
export function policyName(
  t: TFunction<'notify'>,
  policy: Pick<NotifyPolicy, 'name' | 'builtinKey'>,
): string {
  return policy.builtinKey
    ? t(`policies.builtin.${policy.builtinKey}`, { defaultValue: policy.name })
    : policy.name;
}

export function connectorName(
  connectors: NotifyConnector[],
  connectorId: string | null | undefined,
): string {
  return connectors.find((value) => value.id === connectorId)?.name ?? '—';
}

export function endpointLabel(
  endpoint: UserNotifyEndpoint | undefined,
  connectors: NotifyConnector[],
): string {
  if (!endpoint) return '—';
  return `${connectorName(connectors, endpoint.connectorId)} · ${endpoint.displayName ?? endpoint.externalIdentity}`;
}

export function primaryEndpoint(
  preference: UserNotifyPreference | undefined,
  endpoints: UserNotifyEndpoint[],
): UserNotifyEndpoint | undefined {
  // The first high-urgency step: how the person is reached unless an alert is low urgency.
  const first = preference?.steps
    .filter((step) => (step.urgency ?? 'high') === 'high')
    .sort((left, right) => left.stepOrder - right.stepOrder)[0];
  return endpoints.find((endpoint) => endpoint.id === first?.endpointId);
}

export function formatMicros(value: number | null | undefined, locale: string): string {
  if (!value) return '—';
  return new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(value / 1_000));
}

export function statusTone(
  status: NotifyDeliveryStatus | NotifyConnector['status'],
): 'green' | 'red' | 'yellow' | 'blue' | 'dim' {
  if (status === 'success' || status === 'acknowledged' || status === 'connected') {
    return 'green';
  }
  if (status === 'failed' || status === 'error') return 'red';
  if (status === 'sending' || status === 'pending' || status === 'queued') return 'blue';
  if (status === 'unknown') return 'yellow';
  return 'dim';
}

export function targetTypeOptions(): NotifyTargetType[] {
  return ['direct_user', 'fixed_address', 'fixed_group', 'webhook'];
}

export function deliveryStages(): NotifyDeliveryStage[] {
  return [
    'user_primary',
    'user_fallback',
    'team_fallback',
    'organization_fallback',
    'escalation',
    'test',
    'direct',
  ];
}
