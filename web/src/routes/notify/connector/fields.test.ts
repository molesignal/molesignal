import { describe, expect, it } from 'vitest';

import en from '@/i18n/en-us/notify.json';
import zh from '@/i18n/zh-cn/notify.json';

import type { NotifyConnector, NotifyConnectorAddress } from '@/api/notify';

import {
  FIELDS,
  PHONE_CONNECTOR_TYPES,
  buildConfig,
  credentialsComplete,
  defaultTargetType,
  fieldSpecs,
  fieldsFrom,
  regionQuery,
  rowsOf,
  visibleFields,
  withValue,
} from './fields';

const AWS: NotifyConnectorAddress[] = [
  {
    field: 'apiBaseUrl',
    choices: [],
    byRegion: { service: 'sms-voice', domains: ['amazonaws.com', 'amazonaws.com.cn'] },
  },
];

/** The types added with Telegram, Discord, Mattermost, Webex, Matrix, WhatsApp, Amazon SNS and PagerDuty. */
const NEWER_TYPES = [
  'telegram_bot',
  'discord_webhook',
  'mattermost_bot',
  'mattermost_webhook',
  'webex_bot',
  'webex_webhook',
  'matrix_bot',
  'twilio_whatsapp',
  'aws_sns',
  'pagerduty',
];

const aliyun = (service: string): NotifyConnectorAddress[] => [
  {
    field: 'apiBaseUrl',
    choices: [{ key: 'default', url: `https://${service}.aliyuncs.com` }],
    byRegion: { service, domains: ['aliyuncs.com'] },
  },
];

/** The addresses the backend lists for the types these tests open. */
const ADDRESSES: Record<string, NotifyConnectorAddress[]> = {
  aws_sms: AWS,
  aws_voice: AWS,
  aws_sns: [{ ...AWS[0]!, byRegion: { service: 'sns', domains: ['amazonaws.com', 'amazonaws.com.cn'] } }],
  aliyun_sms: aliyun('dysmsapi'),
  aliyun_voice: aliyun('dyvmsapi'),
  lark_app: [
    {
      field: 'apiBaseUrl',
      picker: 'site',
      choices: [
        { key: 'feishu', url: 'https://open.feishu.cn/open-apis' },
        { key: 'lark', url: 'https://open.larksuite.com/open-apis' },
      ],
    },
  ],
  slack_app: [
    {
      field: 'apiBaseUrl',
      picker: 'site',
      choices: [
        { key: 'slack', url: 'https://slack.com/api' },
        { key: 'govslack', url: 'https://slack-gov.com/api' },
      ],
    },
  ],
  teams_app: [
    {
      field: 'serviceUrl',
      picker: 'cloud',
      choices: [
        { key: 'global', url: 'https://smba.trafficmanager.net/teams/' },
        { key: 'gcc', url: 'https://smba.infra.gcc.teams.microsoft.com/teams' },
        { key: 'gcc_high', url: 'https://smba.infra.gov.teams.microsoft.us/teams' },
        { key: 'dod', url: 'https://smba.infra.dod.teams.microsoft.us/teams' },
      ],
    },
  ],
  // Microsoft 365 is signed in to in the worldwide cloud only.
  email_smtp: [{ field: 'loginBaseUrl', choices: [{ key: 'default', url: 'https://login.microsoftonline.com' }] }],
  pagerduty: [
    {
      field: 'apiBaseUrl',
      picker: 'region',
      choices: [
        { key: 'us', url: 'https://events.pagerduty.com' },
        { key: 'eu', url: 'https://events.eu.pagerduty.com' },
      ],
    },
  ],
  telegram_bot: [{ field: 'apiBaseUrl', choices: [{ key: 'default', url: 'https://api.telegram.org' }] }],
  webex_bot: [{ field: 'apiBaseUrl', choices: [{ key: 'default', url: 'https://webexapis.com/v1' }] }],
  twilio_whatsapp: [{ field: 'apiBaseUrl', choices: [{ key: 'default', url: 'https://api.twilio.com' }] }],
};

const specsOf = (connectorType: string) => fieldSpecs(connectorType, ADDRESSES[connectorType]);

const keys = (connectorType: string, values: Record<string, string>) =>
  visibleFields(specsOf(connectorType), values).map((field) => field.key);

const saved = (config: Record<string, unknown>) => ({ config }) as unknown as NotifyConnector;

describe('notify connector fields', () => {
  it('asks an AWS connector only for what its way of signing needs', () => {
    expect(keys('aws_sms', { auth: 'node_role' })).toEqual([
      'auth',
      'region',
      'apiBaseUrl',
      'originationIdentity',
      'timeoutSecs',
      'replies',
    ]);
    expect(keys('aws_sms', { auth: 'temporary_key' })).toContain('sessionToken');
    expect(keys('aws_voice', { auth: 'web_identity' })).toContain('roleArn');
    expect(keys('aws_voice', { auth: 'web_identity' })).not.toContain('secretKey');
    // The external ID shows beside the role it is assumed with, and is never saved.
    expect(keys('aws_voice', { auth: 'assume_role' })).toContain('externalId');
    expect(
      buildConfig(specsOf('aws_voice'), { ...fieldsFrom(specsOf('aws_voice')), auth: 'assume_role' }),
    ).not.toHaveProperty('externalId');
  });

  it('saves the fields shown and leaves a hidden one out', () => {
    const values = {
      ...fieldsFrom(specsOf('aws_sms')),
      auth: 'node_role',
      accessKey: 'AKIA-typed-before',
      region: 'eu-west-1',
    };

    expect(buildConfig(specsOf('aws_sms'), values)).toEqual({
      region: 'eu-west-1',
      auth: 'node_role',
      originationIdentity: '',
      apiBaseUrl: '',
      timeoutSecs: 10,
      replies: false,
    });
  });

  it('needs a whole access key, which a role may also be assumed without', () => {
    const key = { accessKey: 'AKIA', secretKey: 'secret' };

    expect(credentialsComplete('aws_sms', { auth: 'access_key', accessKey: 'AKIA' })).toBe(false);
    expect(credentialsComplete('aws_sms', { auth: 'access_key', ...key })).toBe(true);
    expect(credentialsComplete('aws_voice', { auth: 'assume_role' })).toBe(true);
    expect(credentialsComplete('aws_voice', { auth: 'assume_role', accessKey: 'AKIA' })).toBe(false);
    expect(credentialsComplete('aws_voice', { auth: 'node_role' })).toBe(true);
  });

  it('keeps the Alibaba Cloud templates out of the connector: they are written into notify templates', () => {
    expect(fieldsFrom(specsOf('aliyun_voice'))).toMatchObject({
      playTimes: '2',
      apiBaseUrl: 'https://dyvmsapi.aliyuncs.com',
    });
    expect(Object.keys(buildConfig(specsOf('aliyun_sms'), fieldsFrom(specsOf('aliyun_sms')))).sort()).toEqual([
      'accessKeyId',
      'accessKeySecret',
      'apiBaseUrl',
      'region',
      'replies',
      'signName',
      'timeoutSecs',
    ]);
  });

  it('keeps no provider address of its own: each comes from the backend, shown and not edited', () => {
    for (const [connectorType, specs] of Object.entries(FIELDS)) {
      for (const field of specs) {
        expect(field.defaultValue ?? '', `${connectorType}.${field.key}`).not.toMatch(/^(https?|wss?):\/\//);
        // Huawei Cloud gives each application its own address, and no API lists it: it is copied in.
        if (connectorType.startsWith('huawei_')) continue;
        if (['apiBaseUrl', 'serviceUrl', 'wsUrl', 'pubsubBaseUrl'].includes(field.key)) {
          expect(field.type, `${connectorType}.${field.key}`).toBe('fixed');
        }
      }
    }
    expect(FIELDS.huawei_sms!.find((field) => field.key === 'apiBaseUrl')?.type).toBeUndefined();

    // An address the backend does not list is left out; one it lists without a place here goes last.
    expect(fieldSpecs('dingtalk_app').map((field) => field.key)).not.toContain('apiBaseUrl');
    const listed = fieldSpecs('dingtalk_app', [
      { field: 'apiBaseUrl', choices: [{ key: 'default', url: 'https://api.dingtalk.com' }] },
      { field: 'stream_url', choices: [{ key: 'default', url: 'wss://stream.example.com' }] },
    ]);
    expect(listed.find((field) => field.key === 'apiBaseUrl')).toMatchObject({
      type: 'fixed',
      defaultValue: 'https://api.dingtalk.com',
    });
    expect(listed.at(-1)).toMatchObject({ key: 'stream_url', type: 'fixed' });
  });

  it('picks the address of a Teams cloud or of GovSlack, and reads the choice back', () => {
    const teams = fieldsFrom(specsOf('teams_app'));
    expect(teams).toMatchObject({ cloud: 'global', serviceUrl: 'https://smba.trafficmanager.net/teams/' });
    expect(withValue(specsOf('teams_app'), teams, 'cloud', 'gcc_high').serviceUrl).toBe(
      'https://smba.infra.gov.teams.microsoft.us/teams',
    );

    const dod = saved({ serviceUrl: 'https://smba.infra.dod.teams.microsoft.us/teams/' });
    expect(fieldsFrom(specsOf('teams_app'), dod).cloud).toBe('dod');
    expect(Object.keys(buildConfig(specsOf('teams_app'), teams))).not.toContain('cloud');

    expect(withValue(specsOf('slack_app'), fieldsFrom(specsOf('slack_app')), 'site', 'govslack').apiBaseUrl).toBe(
      'https://slack-gov.com/api',
    );
  });

  it('reads an address the backend no longer lists as its first, and keeps a regional one', () => {
    const emea = saved({ serviceUrl: 'https://smba.trafficmanager.net/emea/' });
    expect(fieldsFrom(specsOf('teams_app'), emea)).toMatchObject({
      cloud: 'global',
      serviceUrl: 'https://smba.trafficmanager.net/teams/',
    });

    const singapore = saved({ region: 'ap-southeast-1', apiBaseUrl: 'https://dysmsapi.ap-southeast-1.aliyuncs.com' });
    expect(fieldsFrom(specsOf('aliyun_sms'), singapore).apiBaseUrl).toBe(
      'https://dysmsapi.ap-southeast-1.aliyuncs.com',
    );
  });

  it('chooses an Alibaba Cloud or AWS region from the ones the provider lists, over the address it decides', () => {
    for (const connectorType of ['aliyun_voice', 'aliyun_sms', 'aws_voice', 'aws_sms']) {
      expect(FIELDS[connectorType]!.find((field) => field.key === 'region')?.type, connectorType).toBe('region');

      // Each on a row of its own, the address right under the region.
      const values = { ...fieldsFrom(specsOf(connectorType)), auth: 'temporary_key' };
      const rows = rowsOf(visibleFields(specsOf(connectorType), values)).map((row) => row.map((field) => field.key));
      const region = rows.findIndex((row) => row[0] === 'region');
      expect(rows.slice(region, region + 2), connectorType).toEqual([['region'], ['apiBaseUrl']]);
    }
    expect(fieldsFrom(specsOf('aliyun_sms')).region).toBe('cn-hangzhou');
  });

  it('picks the Feishu or Lark address with a platform that is not saved', () => {
    const lark = saved({ apiBaseUrl: 'https://open.larksuite.com/open-apis' });

    expect(fieldsFrom(specsOf('lark_app'), lark).site).toBe('lark');
    expect(fieldsFrom(specsOf('lark_app')).site).toBe('feishu');

    const picked = withValue(specsOf('lark_app'), fieldsFrom(specsOf('lark_app')), 'site', 'lark');
    expect(picked.apiBaseUrl).toBe('https://open.larksuite.com/open-apis');
    expect(buildConfig(specsOf('lark_app'), picked)).not.toHaveProperty('site');
    expect(buildConfig(specsOf('lark_app'), picked)).toMatchObject({
      apiBaseUrl: 'https://open.larksuite.com/open-apis',
    });
  });

  it('asks for AWS regions once the way of signing has what it needs', () => {
    const query = (connectorType: string, values: Record<string, string>) =>
      regionQuery(connectorType, specsOf(connectorType), values);

    expect(query('aliyun_voice', {})).toEqual({});
    expect(query('slack_app', {})).toBeNull();
    expect(query('aws_sms', { auth: 'access_key', accessKey: 'AKIA' })).toBeNull();
    expect(query('aws_sms', { auth: 'access_key', accessKey: 'AKIA', secretKey: '***' })).toEqual({
      auth: 'access_key',
      accessKey: 'AKIA',
      secretKey: '***',
    });
    expect(query('aws_voice', { auth: 'node_role', accessKey: 'typed-before' })).toEqual({ auth: 'node_role' });
    expect(query('aws_voice', { auth: 'assume_role' })).toBeNull();
  });

  it('asks for Tencent Cloud regions once both keys are in, and for Huawei Cloud none', () => {
    const query = (connectorType: string, values: Record<string, string>) =>
      regionQuery(connectorType, specsOf(connectorType), values);

    expect(query('tencent_sms', { secretId: 'AKID' })).toBeNull();
    expect(query('tencent_voice', { secretId: ' AKID ', secretKey: ' s ' })).toEqual({ secretId: 'AKID', secretKey: 's' });
    expect(query('huawei_sms', { appKey: 'k', appSecret: 's' })).toBeNull();
  });

  it('saves the replies switch as a boolean and reads it back', () => {
    const specs = fieldSpecs('aws_sms', ADDRESSES.aws_sms);
    const off = fieldsFrom(specs);

    expect(off.replies).toBe('false');
    expect(visibleFields(specs, off).some((field) => field.key === 'replyTopicArn')).toBe(false);
    expect(buildConfig(specs, off).replies).toBe(false);

    const on = { ...off, replies: 'true', replyTopicArn: 'arn:aws:sns:us-east-1:1:replies' };
    expect(buildConfig(specs, on)).toMatchObject({ replies: true, replyTopicArn: 'arn:aws:sns:us-east-1:1:replies' });

    expect(fieldsFrom(specs, saved({ replies: true })).replies).toBe('true');
  });

  it('takes keypresses on an Alibaba Cloud call only from a real number', () => {
    const values = { replies: 'true', showNumber: '' };

    expect(credentialsComplete('aliyun_voice', values)).toBe(false);
    expect(credentialsComplete('aliyun_voice', { ...values, showNumber: '057188773344' })).toBe(true);
    expect(credentialsComplete('aliyun_voice', { ...values, replies: 'false' })).toBe(true);
  });

  it('names every type and each of its fields in both languages, and says what the newer types need', () => {
    for (const pack of [zh, en]) {
      const drawer = pack.connectors.drawer as {
        field: Record<string, string>;
        hints: Record<string, string>;
        type_fields: Record<string, Record<string, string>>;
      };

      for (const connectorType of Object.keys(FIELDS)) {
        expect((pack.connector_types as Record<string, string>)[connectorType], connectorType).toBeTruthy();

        for (const field of fieldSpecs(connectorType, ADDRESSES[connectorType])) {
          const label = drawer.type_fields[connectorType]?.[field.key] ?? drawer.field[field.key];
          expect(label, `${connectorType}.${field.key}`).toBeTruthy();
        }
      }

      for (const connectorType of [...PHONE_CONNECTOR_TYPES, ...NEWER_TYPES]) {
        expect(FIELDS[connectorType], connectorType).toBeDefined();
        expect(drawer.hints[connectorType], connectorType).toBeTruthy();
      }
    }

    // WhatsApp texts a phone: a person's endpoint on it is a number.
    expect(PHONE_CONNECTOR_TYPES.has('twilio_whatsapp')).toBe(true);
  });

  it('keeps the words of each choice under the key of the field that offers it', () => {
    // The editor looks the words up by the field's own key; words kept under another spelling are
    // never shown, and the choice reads as its raw value.
    const selects = new Set(
      Object.keys(FIELDS).flatMap((connectorType) =>
        fieldSpecs(connectorType, ADDRESSES[connectorType])
          .filter((field) => field.type === 'select')
          .map((field) => field.key),
      ),
    );

    for (const pack of [zh, en]) {
      const options = pack.connectors.drawer.options as Record<string, Record<string, string>>;

      for (const key of Object.keys(options)) expect(selects, key).toContain(key);

      for (const field of fieldSpecs('http_sms').filter((spec) => spec.type === 'select')) {
        for (const option of field.options ?? []) {
          expect(options[field.key]?.[option], `${field.key}.${option}`).toBeTruthy();
        }
      }
    }
  });
});

describe('the newer connector types', () => {
  it('asks a bot for its token, and a self-hosted server for its address', () => {
    expect(keys('telegram_bot', {})).toEqual(['botToken', 'apiBaseUrl', 'timeoutSecs']);
    expect(fieldsFrom(specsOf('telegram_bot')).apiBaseUrl).toBe('https://api.telegram.org');
    expect(keys('mattermost_bot', {})).toEqual(['serverUrl', 'botToken', 'timeoutSecs']);
    expect(keys('matrix_bot', {})).toEqual(['homeserverUrl', 'accessToken', 'timeoutSecs']);
    expect(keys('webex_bot', {})).toEqual(['botToken', 'apiBaseUrl', 'timeoutSecs']);

    // A Discord webhook may post under a name and avatar of its own.
    const discord = specsOf('discord_webhook');
    expect(discord.filter((field) => field.optional).map((field) => field.key)).toEqual(['username', 'avatarUrl']);
    expect(rowsOf(discord).map((row) => row.map((field) => field.key))).toEqual([
      ['webhookUrl'],
      ['username', 'avatarUrl'],
      ['timeoutSecs'],
    ]);
  });

  it('sends a test of what posts to one place to a group', () => {
    for (const connectorType of ['webhook', 'pagerduty', 'aws_sns', 'discord_webhook', 'mattermost_webhook']) {
      expect(defaultTargetType(connectorType), connectorType).toBe('fixed_group');
    }
    expect(defaultTargetType('telegram_bot')).toBe('direct_user');
    expect(defaultTargetType('twilio_whatsapp')).toBe('direct_user');
    expect(defaultTargetType('email_smtp')).toBe('fixed_address');
  });

  it('picks the PagerDuty service region, which only sets the address', () => {
    const specs = specsOf('pagerduty');
    const values = fieldsFrom(specs);

    expect(values).toMatchObject({ region: 'us', apiBaseUrl: 'https://events.pagerduty.com' });
    expect(rowsOf(specs).map((row) => row.map((field) => field.key))).toEqual([
      ['routingKey', 'region'],
      ['apiBaseUrl'],
      ['timeoutSecs'],
    ]);

    const eu = withValue(specs, values, 'region', 'eu');
    expect(buildConfig(specs, eu)).toEqual({
      routingKey: '',
      apiBaseUrl: 'https://events.eu.pagerduty.com',
      timeoutSecs: 10,
    });
    expect(fieldsFrom(specs, saved({ apiBaseUrl: 'https://events.eu.pagerduty.com' })).region).toBe('eu');
  });

  it('signs Amazon SNS in as the other AWS connectors do, and sends the event or the template', () => {
    expect(keys('aws_sns', { auth: 'node_role' })).toEqual(['auth', 'region', 'apiBaseUrl', 'topicArn', 'body', 'timeoutSecs']);
    expect(fieldsFrom(specsOf('aws_sns')).body).toBe('event');
    expect(credentialsComplete('aws_sns', { auth: 'access_key', accessKey: 'AKIA' })).toBe(false);
    expect(regionQuery('aws_sns', specsOf('aws_sns'), { auth: 'node_role' })).toEqual({ auth: 'node_role' });
  });
});

describe('email sign-in', () => {
  const specs = specsOf('email_smtp');

  it('asks for a password, or for the Microsoft 365 app, or for the Google service account', () => {
    expect(fieldsFrom(specs).auth).toBe('password');
    expect(keys('email_smtp', { auth: 'password' })).toEqual([
      'host',
      'port',
      'auth',
      'username',
      'password',
      'from',
      'tls',
      'timeoutSecs',
    ]);
    // The sign-in address shows, not edited, for Microsoft alone.
    expect(keys('email_smtp', { auth: 'microsoft' })).toEqual([
      'host',
      'port',
      'auth',
      'tenantId',
      'clientId',
      'clientSecret',
      'loginBaseUrl',
      'username',
      'from',
      'tls',
      'timeoutSecs',
    ]);
    expect(specs.find((field) => field.key === 'loginBaseUrl')).toMatchObject({
      type: 'fixed',
      defaultValue: 'https://login.microsoftonline.com',
    });
    expect(keys('email_smtp', { auth: 'google' })).toEqual([
      'host',
      'port',
      'auth',
      'credentials',
      'username',
      'from',
      'tls',
      'timeoutSecs',
    ]);
  });

  it('sends with OAuth as a mailbox, named as the user name', () => {
    expect(credentialsComplete('email_smtp', { auth: 'password', username: '' })).toBe(true);
    expect(credentialsComplete('email_smtp', { auth: 'microsoft', username: ' ' })).toBe(false);
    expect(credentialsComplete('email_smtp', { auth: 'google', username: 'alerts@example.com' })).toBe(true);
  });

  it('saves the Microsoft sign-in address with Microsoft, and nothing of the other ways', () => {
    const values = { ...fieldsFrom(specs), auth: 'microsoft', password: 'typed-before', tenantId: 't', clientId: 'c' };
    const config = buildConfig(specs, values);

    expect(config).toMatchObject({ auth: 'microsoft', loginBaseUrl: 'https://login.microsoftonline.com' });
    expect(config).not.toHaveProperty('password');
    expect(config).not.toHaveProperty('credentials');

    // A connector saved before sign-in modes signs in with its password.
    const older = fieldsFrom(specs, saved({ host: 'smtp.example.com', username: 'u', password: '***' }));
    expect(older.auth).toBe('password');
    expect(buildConfig(specs, older)).not.toHaveProperty('loginBaseUrl');
  });
});
