import type * as notifyApi from '@/api/notify';

import { channelOf } from '../channels';
import { MAILBOX_AUTH_MODES } from './email';
import type { FieldSpec } from './spec';

/** The types that sign in to AWS, each the same way. */
const AWS_TYPES: ReadonlySet<string> = new Set(['aws_sms', 'aws_voice', 'aws_sns']);

/** Without the `/` an address may end in. */
function bare(url: string): string {
  return url.trim().replace(/\/+$/, '');
}

/** The fields shown with these values: one that depends on another shows only while that one holds a value it names. */
export function visibleFields(specs: FieldSpec[], values: Record<string, string>): FieldSpec[] {
  return specs.filter((field) => !field.when || field.when.values.includes(values[field.when.key] ?? ''));
}

/** Credentials that only make sense together. A Slack app sends with its bot
 * token, and adds status pages to channels with its client ID and secret: it
 * needs one of the two, and the client ID and secret come together. An AWS
 * access key is an ID and a secret, which a role may also be assumed without.
 * An Alibaba Cloud call that takes keypresses is placed from a real number.
 * Email signed in with OAuth sends as a mailbox, named as the user name. */
export function credentialsComplete(connectorType: string, values: Record<string, string>): boolean {
  const filled = (key: string) => (values[key] ?? '').trim() !== '';

  if (connectorType === 'slack_app') {
    if (filled('clientId') !== filled('clientSecret')) return false;
    return filled('botToken') || filled('clientId');
  }

  if (connectorType === 'aliyun_voice' && values.replies === 'true' && !filled('showNumber')) return false;

  if (connectorType === 'email_smtp' && MAILBOX_AUTH_MODES.includes(values.auth ?? '')) return filled('username');

  if (AWS_TYPES.has(connectorType)) {
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

  if (!AWS_TYPES.has(connectorType)) return null;

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

/** The target a test of this type is most likely sent to: a group for what posts to one place (a
 * webhook, an incident service, a topic), an address for email, a person for the rest. */
export function defaultTargetType(connectorType: string): notifyApi.NotifyTargetType {
  if (channelOf(connectorType) === 'webhook' || connectorType.endsWith('_webhook')) return 'fixed_group';
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
