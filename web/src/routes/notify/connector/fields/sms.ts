import { AWS_SETTINGS, REPLIES, TIMEOUT, address, type FieldSpec } from './spec';

/** The placeholders an HTTP SMS request is written with, as the backend fills them. */
export const HTTP_SMS_PLACEHOLDERS = ['phone', 'content', 'title', 'text', 'secret'] as const;

/** The settings of the types that text a phone: SMS, and WhatsApp messages through Twilio. */
export const SMS_FIELDS: Record<string, FieldSpec[]> = {
  aliyun_sms: [
    { key: 'accessKeyId' },
    { key: 'accessKeySecret', type: 'password' },
    { key: 'region', type: 'region', defaultValue: 'cn-hangzhou' },
    address('apiBaseUrl'),
    { key: 'signName' },
    TIMEOUT,
    REPLIES,
  ],
  tencent_sms: [
    { key: 'secretId' },
    { key: 'secretKey', type: 'password' },
    { key: 'region', type: 'region', defaultValue: 'ap-guangzhou' },
    address('apiBaseUrl'),
    { key: 'sdkAppId' },
    { key: 'signName', optional: true },
    TIMEOUT,
    REPLIES,
  ],
  // The address goes with the application and can only be copied from the console: no API lists it.
  huawei_sms: [
    { key: 'appKey' },
    { key: 'appSecret', type: 'password' },
    { key: 'apiBaseUrl', wide: true },
    { key: 'sender' },
    { key: 'signature', optional: true },
    TIMEOUT,
    REPLIES,
  ],
  twilio_sms: [
    { key: 'accountSid' },
    { key: 'authToken', type: 'password' },
    { key: 'sender' },
    address('apiBaseUrl'),
    TIMEOUT,
    REPLIES,
  ],
  // WhatsApp sends only templates it approved: they are written into SMS notify templates.
  twilio_whatsapp: [
    { key: 'accountSid' },
    { key: 'authToken', type: 'password' },
    { key: 'sender' },
    address('apiBaseUrl'),
    TIMEOUT,
    REPLIES,
  ],
  aws_sms: [
    ...AWS_SETTINGS,
    { key: 'originationIdentity', optional: true },
    TIMEOUT,
    REPLIES,
    { key: 'replyTopicArn', optional: true, wide: true, when: { key: 'replies', values: ['true'] } },
  ],
  http_sms: [
    { key: 'url', wide: true },
    { key: 'method', type: 'select', options: ['post', 'put', 'get'], defaultValue: 'post' },
    {
      key: 'bodyFormat',
      type: 'select',
      options: ['json', 'form'],
      defaultValue: 'json',
      when: { key: 'method', values: ['post', 'put'] },
    },
    {
      key: 'body',
      type: 'textarea',
      defaultValue: '{"phone": "{{phone}}", "content": "{{content}}"}',
      when: { key: 'method', values: ['post', 'put'] },
    },
    { key: 'headers', type: 'json', defaultValue: '{}', optional: true },
    { key: 'secret', type: 'password', optional: true },
    { key: 'phoneFormat', type: 'select', options: ['e164', 'digits', 'mainland'], defaultValue: 'e164' },
    { key: 'successField', optional: true },
    { key: 'successValue', optional: true },
    TIMEOUT,
    REPLIES,
  ],
};
