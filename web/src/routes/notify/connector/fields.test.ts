import { describe, expect, it } from 'vitest';

import zh from '@/i18n/zh-cn/notify.json';

import type { NotifyConnector, NotifyConnectorAddress } from '@/api/notify';

import {
  FIELDS,
  PHONE_CONNECTOR_TYPES,
  buildConfig,
  credentialsComplete,
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

  it('names every phone connector and each of its fields', () => {
    const drawer = zh.connectors.drawer as {
      field: Record<string, string>;
      hints: Record<string, string>;
    };

    for (const connectorType of PHONE_CONNECTOR_TYPES) {
      expect(FIELDS[connectorType], connectorType).toBeDefined();
      expect((zh.connector_types as Record<string, string>)[connectorType], connectorType).toBeTruthy();
      expect(drawer.hints[connectorType], connectorType).toBeTruthy();

      for (const field of FIELDS[connectorType]!) {
        expect(drawer.field[field.key], `${connectorType}.${field.key}`).toBeTruthy();
      }
    }
  });
});
