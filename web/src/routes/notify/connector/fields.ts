import type * as notifyApi from '@/api/notify';

/** One setting a connector type asks for. `external_id` is not a setting: it shows the organization's
 * AWS external ID beside the role it is required by, and is never saved. A `fixed` setting holds one of
 * the provider's addresses, which the backend lists: it is shown but not edited. A `region` is chosen
 * from the regions the provider lists, and sets the address to that region's. A `switch` is on or off,
 * saved as `true` or `false`. */
export interface FieldSpec {
  key: string;
  type?:
    | 'text'
    | 'number'
    | 'password'
    | 'json'
    | 'textarea'
    | 'select'
    | 'switch'
    | 'external_id'
    | 'fixed'
    | 'region';
  options?: string[];
  defaultValue?: string;
  optional?: boolean;
  /** Takes a row of its own, as a choice whose meaning is spelled out under it does. */
  wide?: boolean;
  /** Shown, saved and checked only while another field holds one of these values. */
  when?: { key: string; values: string[] };
  /** A choice that is not saved: each option stands for a value of another field, which it sets and
   * is read back from, as Feishu or Lark picks the API base URL. */
  picks?: { key: string; values: Record<string, string> };
  /** The addresses a `fixed` setting may hold, and whether it follows the chosen region instead. */
  addresses?: { urls: string[]; byRegion: boolean };
}

/** The field a chosen region sets to that region's address. */
export const REGION_SETS = 'apiBaseUrl';

/** Where a provider's address shows among a type's settings. The address, and the choice that picks
 * one of several, come from the addresses the backend lists for the type: see {@link fieldSpecs}. */
function address(key: string): FieldSpec {
  return { key, type: 'fixed' };
}

/** The ways an AWS connector proves who it is, the same as the data connectors to AWS. */
const AWS_AUTH_MODES = ['access_key', 'temporary_key', 'node_role', 'assume_role', 'web_identity'];

/** The modes an access key is entered in; with `assume_role` it is the optional key the role is assumed with. */
const AWS_KEY_MODES = ['access_key', 'temporary_key', 'assume_role'];

const AWS_SETTINGS: FieldSpec[] = [
  { key: 'auth', type: 'select', options: AWS_AUTH_MODES, defaultValue: 'access_key', wide: true },
  { key: 'accessKey', optional: true, when: { key: 'auth', values: AWS_KEY_MODES } },
  { key: 'secretKey', type: 'password', optional: true, when: { key: 'auth', values: AWS_KEY_MODES } },
  { key: 'sessionToken', type: 'password', when: { key: 'auth', values: ['temporary_key'] } },
  { key: 'roleArn', when: { key: 'auth', values: ['assume_role', 'web_identity'] } },
  { key: 'externalId', type: 'external_id', optional: true, when: { key: 'auth', values: ['assume_role'] } },
  { key: 'region', type: 'region', defaultValue: 'us-east-1' },
  address('apiBaseUrl'),
];

const TIMEOUT: FieldSpec = { key: 'timeoutSecs', type: 'number', defaultValue: '10' };

/** Replies to a text, or keys pressed on a call, acknowledge or resolve the alerts in it. */
const REPLIES: FieldSpec = { key: 'replies', type: 'switch', defaultValue: 'false', wide: true };

/** Connector types that call or text a phone number: a person's endpoint is the number. */
export const PHONE_CONNECTOR_TYPES = new Set([
  'aliyun_voice',
  'aliyun_sms',
  'tencent_voice',
  'tencent_sms',
  'huawei_voice',
  'huawei_sms',
  'twilio_voice',
  'twilio_sms',
  'aws_voice',
  'aws_sms',
  'http_sms',
]);

/** The placeholders an HTTP SMS request is written with, as the backend fills them. */
export const HTTP_SMS_PLACEHOLDERS = ['phone', 'content', 'title', 'text', 'secret'] as const;

export const FIELDS: Record<string, FieldSpec[]> = {
  email_smtp: [
    { key: 'host' },
    { key: 'port', type: 'number', defaultValue: '587' },
    { key: 'username', optional: true },
    { key: 'password', type: 'password' },
    { key: 'from' },
    { key: 'tls', type: 'select', options: ['none', 'starttls', 'tls'], defaultValue: 'starttls' },
    TIMEOUT,
  ],
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
  aliyun_sms: [
    { key: 'accessKeyId' },
    { key: 'accessKeySecret', type: 'password' },
    { key: 'region', type: 'region', defaultValue: 'cn-hangzhou' },
    address('apiBaseUrl'),
    { key: 'signName' },
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
  huawei_voice: [
    { key: 'appKey' },
    { key: 'appSecret', type: 'password' },
    { key: 'apiBaseUrl', wide: true },
    { key: 'showNumber' },
    TIMEOUT,
    REPLIES,
  ],
  huawei_sms: [
    { key: 'appKey' },
    { key: 'appSecret', type: 'password' },
    { key: 'apiBaseUrl', wide: true },
    { key: 'sender' },
    { key: 'signature', optional: true },
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
  twilio_sms: [
    { key: 'accountSid' },
    { key: 'authToken', type: 'password' },
    { key: 'sender' },
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
  webhook: [
    { key: 'url', type: 'password' },
    { key: 'method', type: 'select', options: ['post', 'put', 'patch'], defaultValue: 'post' },
    { key: 'body', type: 'select', options: ['event', 'template'], defaultValue: 'event', wide: true },
    { key: 'signingSecret', type: 'password', optional: true },
    { key: 'headers', type: 'json', defaultValue: '{}', optional: true },
    TIMEOUT,
  ],
};

/** Without the `/` an address may end in. */
function bare(url: string): string {
  return url.trim().replace(/\/+$/, '');
}

/** A type's settings with its provider's addresses as the backend lists them: an address starts at the
 * first one listed, and one of several is picked by a choice right above it that is not saved. An address
 * the backend does not list is left out, and one it lists without a place here goes last. */
export function fieldSpecs(
  connectorType: string,
  addresses: notifyApi.NotifyConnectorAddress[] = [],
): FieldSpec[] {
  const placed = FIELDS[connectorType] ?? [];
  const unplaced = addresses
    .filter((listed) => !placed.some((field) => field.type === 'fixed' && field.key === listed.field))
    .map((listed) => address(listed.field));

  return [...placed, ...unplaced].flatMap((field): FieldSpec[] => {
    if (field.type !== 'fixed') return [field];

    const listed = addresses.find((entry) => entry.field === field.key);
    if (!listed) return [];

    const urls = listed.choices.map((choice) => choice.url);
    const spec: FieldSpec = {
      ...field,
      defaultValue: urls[0] ?? '',
      // An address that follows the region may stay empty until a region is chosen.
      optional: urls.length === 0,
      addresses: { urls, byRegion: listed.byRegion != null },
    };

    if (!listed.picker || listed.choices.length < 2) return [spec];

    const picker: FieldSpec = {
      key: listed.picker,
      type: 'select',
      options: listed.choices.map((choice) => choice.key),
      defaultValue: listed.choices[0]!.key,
      picks: {
        key: listed.field,
        values: Object.fromEntries(listed.choices.map((choice) => [choice.key, choice.url])),
      },
    };

    return [picker, spec];
  });
}

/** The fields shown with these values: one that depends on another shows only while that one holds a value it names. */
export function visibleFields(specs: FieldSpec[], values: Record<string, string>): FieldSpec[] {
  return specs.filter((field) => !field.when || field.when.values.includes(values[field.when.key] ?? ''));
}

/** Credentials that only make sense together. A Slack app sends with its bot
 * token, and adds status pages to channels with its client ID and secret: it
 * needs one of the two, and the client ID and secret come together. An AWS
 * access key is an ID and a secret, which a role may also be assumed without.
 * An Alibaba Cloud call that takes keypresses is placed from a real number. */
export function credentialsComplete(connectorType: string, values: Record<string, string>): boolean {
  const filled = (key: string) => (values[key] ?? '').trim() !== '';

  if (connectorType === 'slack_app') {
    if (filled('clientId') !== filled('clientSecret')) return false;
    return filled('botToken') || filled('clientId');
  }

  if (connectorType === 'aliyun_voice' && values.replies === 'true' && !filled('showNumber')) return false;

  if (connectorType === 'aws_voice' || connectorType === 'aws_sms') {
    const mode = values.auth ?? '';
    if (mode === 'access_key' || mode === 'temporary_key') return filled('accessKey') && filled('secretKey');
    if (mode === 'assume_role') return filled('accessKey') === filled('secretKey');
  }

  return true;
}

export function fieldsFrom(
  specs: FieldSpec[],
  connector?: notifyApi.NotifyConnector | null,
): Record<string, string> {
  const values: Record<string, string> = Object.fromEntries(
    specs.map((field) => {
      const stored = connector?.config[field.key];
      const value =
        field.type === 'json' && stored && typeof stored === 'object'
          ? JSON.stringify(stored, null, 2)
          : stored === undefined
            ? field.defaultValue ?? ''
            : String(stored);
      return [field.key, value];
    }),
  );

  // An address the backend no longer lists reads as the first it does, which saving puts in its place.
  for (const field of specs) {
    if (!field.addresses || field.addresses.byRegion) continue;
    const stored = bare(values[field.key] ?? '');
    if (!field.addresses.urls.some((url) => bare(url) === stored)) values[field.key] = field.defaultValue ?? '';
  }

  // A choice that is not saved is read back from the value it picked.
  for (const field of specs) {
    if (!field.picks) continue;
    const picked = bare(values[field.picks.key] ?? '');
    const option = Object.entries(field.picks.values).find(([, value]) => bare(value) === picked)?.[0];
    values[field.key] = option ?? field.defaultValue ?? '';
  }

  return values;
}

/** The values after `key` is set to `value`: a choice that picks another field sets it too. */
export function withValue(
  specs: FieldSpec[],
  values: Record<string, string>,
  key: string,
  value: string,
): Record<string, string> {
  const next = { ...values, [key]: value };
  const picks = specs.find((field) => field.key === key)?.picks;
  const picked = picks?.values[value];
  if (picks && picked !== undefined) next[picks.key] = picked;
  return next;
}

/** The settings a region list is asked with, or `null` while they are not enough to ask: Alibaba
 * Cloud lists its regions to anyone, AWS and Tencent Cloud list them to the account the connector signs in to. */
export function regionQuery(
  connectorType: string,
  specs: FieldSpec[],
  values: Record<string, string>,
): Record<string, string> | null {
  if (connectorType === 'aliyun_sms' || connectorType === 'aliyun_voice') return {};

  if (connectorType === 'tencent_sms' || connectorType === 'tencent_voice') {
    const id = (values.secretId ?? '').trim();
    const key = (values.secretKey ?? '').trim();

    return id !== '' && key !== '' ? { secretId: id, secretKey: key } : null;
  }

  if (connectorType !== 'aws_sms' && connectorType !== 'aws_voice') return null;

  const filled = (key: string) => (values[key] ?? '').trim() !== '';
  const keyed = filled('accessKey') && filled('secretKey');
  const mode = values.auth ?? '';
  const ready =
    mode === 'node_role' ||
    (mode === 'access_key' && keyed) ||
    (mode === 'temporary_key' && keyed && filled('sessionToken')) ||
    (mode === 'assume_role' && filled('roleArn') && filled('accessKey') === filled('secretKey')) ||
    (mode === 'web_identity' && filled('roleArn'));

  if (!ready) return null;

  return Object.fromEntries(
    visibleFields(specs, values)
      .filter((field) => ['auth', 'accessKey', 'secretKey', 'sessionToken', 'roleArn'].includes(field.key))
      .map((field) => [field.key, values[field.key] ?? '']),
  );
}

/** The configuration to save: the settings shown, a hidden one is left out, and so is a choice
 * that only picks another setting. */
export function buildConfig(
  specs: FieldSpec[],
  values: Record<string, string>,
): Record<string, unknown> {
  return Object.fromEntries(
    visibleFields(specs, values)
      .filter((field) => field.type !== 'external_id' && !field.picks)
      .map((field) => {
        const value = values[field.key] ?? '';
        if (field.type === 'number') return [field.key, Number(value)];
        if (field.type === 'switch') return [field.key, value === 'true'];
        if (field.type === 'json') {
          return [
            field.key,
            value.trim() === '***' ? '***' : (JSON.parse(value) as unknown),
          ];
        }
        return [field.key, value];
      }),
  );
}

/** The target a test of this type is most likely sent to. */
export function defaultTargetType(connectorType: string): notifyApi.NotifyTargetType {
  if (connectorType === 'webhook' || connectorType.endsWith('_webhook')) return 'fixed_group';
  return connectorType === 'email_smtp' ? 'fixed_address' : 'direct_user';
}

/** The fields two to a row. A JSON field, a wide one and the external ID take a row of their own,
 * and so do a region and an address, which a half row would cut short; the address a region decides
 * comes right under it. */
export function rowsOf(specs: FieldSpec[]): FieldSpec[][] {
  const alone = (field: FieldSpec) =>
    field.type === 'json' ||
    field.type === 'textarea' ||
    field.type === 'external_id' ||
    field.type === 'fixed' ||
    field.type === 'region' ||
    field.wide === true;
  const rows: FieldSpec[][] = [];
  for (const field of specs) {
    const last = rows[rows.length - 1];
    if (!alone(field) && last && last.length === 1 && !alone(last[0]!)) last.push(field);
    else rows.push([field]);
  }
  return rows;
}

/** The first JSON field whose text does not parse; a masked `***` is kept as it is. */
export function unparsedJson(specs: FieldSpec[], values: Record<string, string>): string | null {
  for (const field of visibleFields(specs, values)) {
    const value = (values[field.key] ?? '').trim();
    if (field.type !== 'json' || value === '***' || (field.optional && value === '')) continue;
    try {
      JSON.parse(value);
    } catch {
      return field.key;
    }
  }
  return null;
}
