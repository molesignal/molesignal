import type { NotifyChannel } from '@/api/notify';

/** The channels connector types deliver through, in the order the type picker lists them. */
export const CONNECTOR_CHANNELS: readonly NotifyChannel[] = ['im', 'phone', 'sms', 'email', 'webhook'];

/** A type the table below does not know is listed last, under `other`. */
export type ConnectorChannel = NotifyChannel | 'other';

/**
 * The types of each channel. A Chinese interface lists the domestic platforms and vendors first,
 * any other the international ones; a platform's app comes before its webhook. A generic type,
 * which speaks to any vendor, comes last in either language.
 */
const CHANNEL_TYPES: Record<
  NotifyChannel,
  { domestic: string[]; international: string[]; generic?: string[] }
> = {
  im: {
    domestic: ['lark_app', 'lark_webhook', 'dingtalk_app', 'dingtalk_webhook', 'wecom_app', 'wecom_bot', 'wecom_webhook'],
    international: ['slack_app', 'slack_webhook', 'teams_app', 'teams_webhook', 'google_chat_app', 'google_chat_webhook'],
  },
  phone: { domestic: ['aliyun_voice', 'tencent_voice', 'huawei_voice'], international: ['twilio_voice', 'aws_voice'] },
  sms: {
    domestic: ['aliyun_sms', 'tencent_sms', 'huawei_sms'],
    international: ['twilio_sms', 'aws_sms'],
    generic: ['http_sms'],
  },
  email: { domestic: [], international: ['email_smtp'] },
  webhook: { domestic: [], international: ['webhook'] },
};

export interface ChannelType {
  channel: ConnectorChannel;
  connectorType: string;
}

/** The connector types the server offers, by channel and in the interface language's order. */
export function orderConnectorTypes(offered: readonly string[], language: string): ChannelType[] {
  const domesticFirst = language.toLowerCase().startsWith('zh');
  const known = new Set<string>();
  const ordered: ChannelType[] = [];

  for (const channel of CONNECTOR_CHANNELS) {
    const { domestic, international, generic = [] } = CHANNEL_TYPES[channel];
    const local = domesticFirst ? [...domestic, ...international] : [...international, ...domestic];
    for (const connectorType of [...local, ...generic]) {
      known.add(connectorType);
      if (offered.includes(connectorType)) ordered.push({ channel, connectorType });
    }
  }

  for (const connectorType of offered) {
    if (!known.has(connectorType)) ordered.push({ channel: 'other', connectorType });
  }

  return ordered;
}

/** The channel a connector type delivers through, or null for a type the table does not know. */
export function channelOf(connectorType: string): NotifyChannel | null {
  return (
    CONNECTOR_CHANNELS.find((channel) => {
      const { domestic, international, generic = [] } = CHANNEL_TYPES[channel];
      return [...domestic, ...international, ...generic].includes(connectorType);
    }) ?? null
  );
}
