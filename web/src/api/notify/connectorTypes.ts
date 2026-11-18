import { http } from '@/lib/http';

import type { ConnectorCapabilities } from './index';

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
  twilio_whatsapp: 'whatsapp',
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
  'telegram_bot',
  'discord_webhook',
  'mattermost_bot',
  'mattermost_webhook',
  'webex_bot',
  'webex_webhook',
  'matrix_bot',
]);

export function sendsCards(connectorType: string): boolean {
  return CARD_CONNECTOR_TYPES.has(connectorType);
}

export interface NotifyConnectorType {
  connectorType: string;
  capabilities: ConnectorCapabilities;
  /** The provider's addresses the type's settings keep; the backend saves no others. */
  addresses?: NotifyConnectorAddress[];
  /** How the IM app can page harder once a step's message is sent: `app` (in the app), `sms` or
   * `phone`. A step of a person's notification order picks one or none; most types offer none. */
  boosts?: string[];
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
