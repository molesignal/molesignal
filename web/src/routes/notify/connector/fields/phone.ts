import { AWS_SETTINGS, REPLIES, TIMEOUT, address, type FieldSpec } from './spec';

/** The settings of the types that call a phone and read the notification out. */
export const PHONE_FIELDS: Record<string, FieldSpec[]> = {
  aliyun_voice: [
    { key: 'accessKeyId' },
    { key: 'accessKeySecret', type: 'password' },
    { key: 'region', type: 'region', defaultValue: 'cn-hangzhou' },
    address('apiBaseUrl'),
    { key: 'showNumber', optional: true },
    { key: 'playTimes', type: 'number', defaultValue: '2' },
    TIMEOUT,
    REPLIES,
  ],
  tencent_voice: [
    { key: 'secretId' },
    { key: 'secretKey', type: 'password' },
    { key: 'region', type: 'region', defaultValue: 'ap-guangzhou' },
    address('apiBaseUrl'),
    { key: 'sdkAppId' },
    { key: 'playTimes', type: 'number', defaultValue: '2' },
    TIMEOUT,
    REPLIES,
  ],
  // The address goes with the application and can only be copied from the console: no API lists it.
  huawei_voice: [
    { key: 'appKey' },
    { key: 'appSecret', type: 'password' },
    { key: 'apiBaseUrl', wide: true },
    { key: 'showNumber' },
    TIMEOUT,
    REPLIES,
  ],
  twilio_voice: [
    { key: 'accountSid' },
    { key: 'authToken', type: 'password' },
    { key: 'fromNumber' },
    { key: 'language', defaultValue: 'en-US' },
    { key: 'voice', optional: true },
    { key: 'playTimes', type: 'number', defaultValue: '2' },
    address('apiBaseUrl'),
    TIMEOUT,
    REPLIES,
  ],
  aws_voice: [
    ...AWS_SETTINGS,
    { key: 'originationIdentity' },
    { key: 'voiceId', optional: true },
    { key: 'playTimes', type: 'number', defaultValue: '2' },
    TIMEOUT,
  ],
};
