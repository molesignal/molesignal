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
 * one of several, come from the addresses the backend lists for the type: see `fieldSpecs`. An
 * address used by one way of signing in shows only with it, and so does its choice. */
export function address(key: string, when?: FieldSpec['when']): FieldSpec {
  return { key, type: 'fixed', ...(when ? { when } : {}) };
}

export const TIMEOUT: FieldSpec = { key: 'timeoutSecs', type: 'number', defaultValue: '10' };

/** Replies to a text, or keys pressed on a call, acknowledge or resolve the alerts in it. */
export const REPLIES: FieldSpec = { key: 'replies', type: 'switch', defaultValue: 'false', wide: true };

/** The ways an AWS connector proves who it is, the same as the data connectors to AWS. */
const AWS_AUTH_MODES = ['access_key', 'temporary_key', 'node_role', 'assume_role', 'web_identity'];

/** The modes an access key is entered in; with `assume_role` it is the optional key the role is assumed with. */
const AWS_KEY_MODES = ['access_key', 'temporary_key', 'assume_role'];

/** How every AWS notify connector signs in, and the region it sends in. */
export const AWS_SETTINGS: FieldSpec[] = [
  { key: 'auth', type: 'select', options: AWS_AUTH_MODES, defaultValue: 'access_key', wide: true },
  { key: 'accessKey', optional: true, when: { key: 'auth', values: AWS_KEY_MODES } },
  { key: 'secretKey', type: 'password', optional: true, when: { key: 'auth', values: AWS_KEY_MODES } },
  { key: 'sessionToken', type: 'password', when: { key: 'auth', values: ['temporary_key'] } },
  { key: 'roleArn', when: { key: 'auth', values: ['assume_role', 'web_identity'] } },
  { key: 'externalId', type: 'external_id', optional: true, when: { key: 'auth', values: ['assume_role'] } },
  { key: 'region', type: 'region', defaultValue: 'us-east-1' },
  address('apiBaseUrl'),
];
