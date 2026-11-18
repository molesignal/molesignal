import type * as notifyApi from '@/api/notify';

import { EMAIL_FIELDS } from './email';
import { IM_FIELDS } from './im';
import { PHONE_FIELDS } from './phone';
import { SMS_FIELDS } from './sms';
import { address, type FieldSpec } from './spec';
import { WEBHOOK_FIELDS } from './webhook';

export * from './form';
export { HTTP_SMS_PLACEHOLDERS } from './sms';
export { REGION_SETS, type FieldSpec } from './spec';

/** The settings of every connector type, by channel. */
export const FIELDS: Record<string, FieldSpec[]> = {
  ...EMAIL_FIELDS,
  ...IM_FIELDS,
  ...PHONE_FIELDS,
  ...SMS_FIELDS,
  ...WEBHOOK_FIELDS,
};

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
  'twilio_whatsapp',
  'aws_voice',
  'aws_sms',
  'http_sms',
]);

/** A type's settings with its provider's addresses as the backend lists them: an address starts at the
 * first one listed, and one of several is picked by a choice right above it that is not saved, shown
 * while the address is. An address the backend does not list is left out, and one it lists without a
 * place here goes last. */
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
      ...(field.when ? { when: field.when } : {}),
    };

    return [picker, spec];
  });
}
