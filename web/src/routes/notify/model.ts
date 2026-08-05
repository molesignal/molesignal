import type {
  NotifyConnector,
  NotifyDeliveryStage,
  NotifyDeliveryStatus,
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
  'oncall.shift.starting',
  'oncall.shift.started',
  'oncall.override.created',
  'oncall.coverage.missing',
] as const;

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
  return `${connectorName(connectors, endpoint.connector_id)} · ${endpoint.display_name ?? endpoint.external_identity}`;
}

export function primaryEndpoint(
  preference: UserNotifyPreference | undefined,
  endpoints: UserNotifyEndpoint[],
): UserNotifyEndpoint | undefined {
  const first = preference?.steps
    .slice()
    .sort((left, right) => left.step_order - right.step_order)[0];
  return endpoints.find((endpoint) => endpoint.id === first?.endpoint_id);
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
  if (status === 'sending' || status === 'pending') return 'blue';
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
  ];
}
