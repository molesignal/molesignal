import { TIMEOUT, address, type FieldSpec } from './spec';

/** The settings of the instant messaging types: platform apps and bots, and group webhooks. */
export const IM_FIELDS: Record<string, FieldSpec[]> = {
  slack_app: [
    { key: 'botToken', type: 'password', optional: true },
    { key: 'appToken', type: 'password', optional: true },
    address('apiBaseUrl'),
    { key: 'clientId', optional: true },
    { key: 'clientSecret', type: 'password', optional: true },
    TIMEOUT,
  ],
  slack_webhook: [{ key: 'webhookUrl', type: 'password' }, TIMEOUT],
  lark_app: [
    { key: 'appId' },
    { key: 'appSecret', type: 'password' },
    address('apiBaseUrl'),
    {
      key: 'receiveIdType',
      type: 'select',
      options: ['open_id', 'user_id', 'union_id', 'email'],
      defaultValue: 'open_id',
    },
    TIMEOUT,
  ],
  lark_webhook: [
    { key: 'webhookUrl', type: 'password' },
    { key: 'secret', type: 'password', optional: true },
    TIMEOUT,
  ],
  dingtalk_app: [
    { key: 'clientId' },
    { key: 'clientSecret', type: 'password' },
    { key: 'robotCode', optional: true },
    { key: 'cardTemplateId', optional: true },
    address('apiBaseUrl'),
    TIMEOUT,
  ],
  dingtalk_webhook: [
    { key: 'webhookUrl', type: 'password' },
    { key: 'secret', type: 'password', optional: true },
    TIMEOUT,
  ],
  wecom_app: [
    { key: 'corpId' },
    { key: 'corpSecret', type: 'password' },
    { key: 'agentId', type: 'number' },
    { key: 'callbackToken', type: 'password', optional: true },
    { key: 'callbackAesKey', type: 'password', optional: true },
    address('apiBaseUrl'),
    TIMEOUT,
  ],
  wecom_webhook: [{ key: 'webhookUrl', type: 'password' }, TIMEOUT],
  wecom_bot: [
    { key: 'botId' },
    { key: 'botSecret', type: 'password' },
    address('wsUrl'),
  ],
  teams_app: [
    { key: 'appId' },
    { key: 'clientSecret', type: 'password' },
    { key: 'tenantId' },
    address('serviceUrl'),
    TIMEOUT,
  ],
  teams_webhook: [{ key: 'webhookUrl', type: 'password' }, TIMEOUT],
  google_chat_app: [
    { key: 'credentials', type: 'json' },
    { key: 'subscription', optional: true },
    address('apiBaseUrl'),
    address('pubsubBaseUrl'),
    TIMEOUT,
  ],
  google_chat_webhook: [{ key: 'webhookUrl', type: 'password' }, TIMEOUT],
  telegram_bot: [{ key: 'botToken', type: 'password' }, address('apiBaseUrl'), TIMEOUT],
  // The display name and avatar replace the webhook's own for its messages.
  discord_webhook: [
    { key: 'webhookUrl', type: 'password', wide: true },
    { key: 'username', optional: true },
    { key: 'avatarUrl', optional: true },
    TIMEOUT,
  ],
  // A server of the organization's own: its address is entered, not listed.
  mattermost_bot: [
    { key: 'serverUrl', wide: true },
    { key: 'botToken', type: 'password' },
    TIMEOUT,
  ],
  mattermost_webhook: [{ key: 'webhookUrl', type: 'password' }, TIMEOUT],
  webex_bot: [{ key: 'botToken', type: 'password' }, address('apiBaseUrl'), TIMEOUT],
  webex_webhook: [{ key: 'webhookUrl', type: 'password' }, TIMEOUT],
  matrix_bot: [
    { key: 'homeserverUrl', wide: true },
    { key: 'accessToken', type: 'password' },
    TIMEOUT,
  ],
};
