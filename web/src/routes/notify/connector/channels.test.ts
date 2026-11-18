import { describe, expect, it } from 'vitest';

import { channelOf, orderConnectorTypes } from './channels';

// As the server lists them: sorted by id.
const OFFERED = [
  'aliyun_sms', 'aliyun_voice', 'aws_sms', 'aws_voice', 'dingtalk_app', 'dingtalk_webhook', 'email_smtp',
  'google_chat_app', 'google_chat_webhook', 'http_sms', 'lark_app', 'lark_webhook', 'slack_app', 'slack_webhook',
  'teams_app', 'teams_webhook', 'twilio_sms', 'twilio_voice', 'webhook', 'wecom_app', 'wecom_bot', 'wecom_webhook',
];

describe('connector types by channel', () => {
  it('lists the domestic platforms first in Chinese, each app before its webhook', () => {
    const ordered = orderConnectorTypes(OFFERED, 'zh-CN');

    expect(ordered.map((item) => item.connectorType)).toEqual([
      'lark_app', 'lark_webhook', 'dingtalk_app', 'dingtalk_webhook', 'wecom_app', 'wecom_bot', 'wecom_webhook',
      'slack_app', 'slack_webhook', 'teams_app', 'teams_webhook', 'google_chat_app', 'google_chat_webhook',
      'aliyun_voice', 'twilio_voice', 'aws_voice',
      'aliyun_sms', 'twilio_sms', 'aws_sms', 'http_sms',
      'email_smtp',
      'webhook',
    ]);
    expect([...new Set(ordered.map((item) => item.channel))]).toEqual(['im', 'phone', 'sms', 'email', 'webhook']);
  });

  it('lists the international ones first in any other language', () => {
    const ordered = orderConnectorTypes(OFFERED, 'en-US').map((item) => item.connectorType);

    expect(ordered[0]).toBe('slack_app');
    expect(ordered.indexOf('lark_app')).toBe(6);
    expect(ordered.slice(13, 16)).toEqual(['twilio_voice', 'aws_voice', 'aliyun_voice']);
    // The generic gateway comes after every vendor, in either language.
    expect(ordered.slice(16, 20)).toEqual(['twilio_sms', 'aws_sms', 'aliyun_sms', 'http_sms']);
  });

  it('lists Tencent Cloud and Huawei Cloud with the domestic vendors, after Alibaba Cloud', () => {
    const offered = [
      'aliyun_sms', 'aliyun_voice', 'huawei_sms', 'huawei_voice', 'http_sms', 'tencent_sms', 'tencent_voice',
      'twilio_sms', 'twilio_voice',
    ];

    expect(orderConnectorTypes(offered, 'zh-CN').map((item) => item.connectorType)).toEqual([
      'aliyun_voice', 'tencent_voice', 'huawei_voice', 'twilio_voice',
      'aliyun_sms', 'tencent_sms', 'huawei_sms', 'twilio_sms', 'http_sms',
    ]);
    expect(orderConnectorTypes(offered, 'en-US').map((item) => item.connectorType)).toEqual([
      'twilio_voice', 'aliyun_voice', 'tencent_voice', 'huawei_voice',
      'twilio_sms', 'aliyun_sms', 'tencent_sms', 'huawei_sms', 'http_sms',
    ]);
    expect(channelOf('tencent_voice')).toBe('phone');
    expect(channelOf('huawei_sms')).toBe('sms');
  });

  it('keeps only what the server offers, and lists an unknown type last', () => {
    expect(orderConnectorTypes(['webhook', 'carrier_pigeon', 'twilio_sms'], 'zh-cn')).toEqual([
      { channel: 'sms', connectorType: 'twilio_sms' },
      { channel: 'webhook', connectorType: 'webhook' },
      { channel: 'other', connectorType: 'carrier_pigeon' },
    ]);
  });

  it('names the channel of a type, and none for a type it does not know', () => {
    expect(channelOf('aliyun_voice')).toBe('phone');
    expect(channelOf('http_sms')).toBe('sms');
    expect(channelOf('lark_app')).toBe('im');
    expect(channelOf('carrier_pigeon')).toBeNull();
  });

  it('lists the newer IM platforms after Google Chat, each bot before its webhook', () => {
    const offered = [
      'discord_webhook', 'google_chat_app', 'lark_app', 'matrix_bot', 'mattermost_bot', 'mattermost_webhook',
      'telegram_bot', 'webex_bot', 'webex_webhook',
    ];

    expect(orderConnectorTypes(offered, 'en-US').map((item) => item.connectorType)).toEqual([
      'google_chat_app', 'telegram_bot', 'discord_webhook', 'mattermost_bot', 'mattermost_webhook', 'webex_bot',
      'webex_webhook', 'matrix_bot', 'lark_app',
    ]);
    expect(orderConnectorTypes(offered, 'zh-CN')[0]?.connectorType).toBe('lark_app');
    expect(new Set(offered.map(channelOf))).toEqual(new Set(['im']));
  });

  it('sends WhatsApp with the texts, and PagerDuty and Amazon SNS with the webhook, which comes last', () => {
    const offered = ['aws_sms', 'aws_sns', 'http_sms', 'pagerduty', 'twilio_sms', 'twilio_whatsapp', 'webhook'];

    expect(orderConnectorTypes(offered, 'zh-CN')).toEqual([
      { channel: 'sms', connectorType: 'twilio_sms' },
      { channel: 'sms', connectorType: 'aws_sms' },
      { channel: 'sms', connectorType: 'twilio_whatsapp' },
      { channel: 'sms', connectorType: 'http_sms' },
      { channel: 'webhook', connectorType: 'pagerduty' },
      { channel: 'webhook', connectorType: 'aws_sns' },
      { channel: 'webhook', connectorType: 'webhook' },
    ]);
  });
});
