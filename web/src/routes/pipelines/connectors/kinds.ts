import type { Connector } from '@/api/connectors';

/**
 * The connector kinds the backend knows (`infra/connectors/mod.rs`) and what
 * each needs to work: where it reads from or delivers to, and how it proves
 * who it is. The keys are the ones the backend reads from `config_json`
 * (`infra/connectors/config.rs`); nothing is offered here that it does not do.
 */
export type ConnectorKind =
  | 'aws_cloudwatch_logs'
  | 'aws_kinesis_firehose'
  | 'cloudflare_logpush'
  | 'heroku_drain'
  | 's3'
  | 'kafka';

/** Whether a connector brings data in, or carries what pipelines write out. */
export type Direction = 'source' | 'destination';

/**
 * How the data moves: fetched by MoleSignal on a timer, sent to it by the
 * other service, or delivered by a pipeline.
 */
export type Transport = 'pull' | 'push' | 'sink';

/**
 * How a connector authenticates.
 *  - `access_key`: a long-lived AWS access key (id and secret);
 *  - `temporary_key`: a short-lived one, which is only valid with its session token;
 *  - `node_role`: no key at all, the role of the machine MoleSignal runs on;
 *  - `sasl_plain`: a Kafka user name and password;
 *  - `none`: nothing, for a broker that asks for nothing;
 *  - `push_token`: a token the other service presents with every push.
 */
export type AuthMode =
  | 'access_key'
  | 'temporary_key'
  | 'node_role'
  | 'sasl_plain'
  | 'none'
  | 'push_token';

export interface KindSpec {
  kind: ConnectorKind;
  direction: Direction;
  transport: Transport;
  /** The ways it can authenticate, the usual one first. */
  auth: readonly AuthMode[];
  /** Plain settings: what to read or where to deliver. Starred ones are required. */
  fields: readonly FieldSpec[];
  /** Where a push connector listens, under the API root. */
  pushPath?: string;
}

export interface FieldSpec {
  key: string;
  required?: boolean;
  /** An example of the value, shown while the field is empty. It is not copy: it reads the same in any language. */
  example?: string;
  /** A stream of this workspace, existing or to be created by the first event. */
  stream?: boolean;
  /** Belongs with the S3-compatible settings, shown only when those are open. */
  compatible?: boolean;
}

const TARGET_STREAM: FieldSpec = { key: 'target_stream', stream: true, example: 'default' };

export const KINDS: readonly KindSpec[] = [
  {
    kind: 'aws_cloudwatch_logs',
    direction: 'source',
    transport: 'pull',
    auth: ['access_key', 'temporary_key'],
    fields: [
      { key: 'region', required: true, example: 'us-east-1' },
      { key: 'log_group', required: true, example: '/aws/lambda/checkout' },
      TARGET_STREAM,
    ],
  },
  {
    kind: 'aws_kinesis_firehose',
    direction: 'source',
    transport: 'push',
    auth: ['push_token'],
    fields: [TARGET_STREAM],
    pushPath: '/api/v1/_kinesis_firehose',
  },
  {
    kind: 'cloudflare_logpush',
    direction: 'source',
    transport: 'push',
    auth: ['push_token'],
    fields: [TARGET_STREAM],
    pushPath: '/api/v1/_cloudflare',
  },
  {
    kind: 'heroku_drain',
    direction: 'source',
    transport: 'push',
    auth: ['push_token'],
    fields: [TARGET_STREAM],
    pushPath: '/api/v1/_heroku',
  },
  {
    kind: 's3',
    direction: 'destination',
    transport: 'sink',
    auth: ['access_key', 'temporary_key', 'node_role'],
    fields: [
      { key: 'bucket', required: true, example: 'acme-logs' },
      { key: 'region', example: 'us-east-1' },
      { key: 'prefix', example: 'pipelines/checkout/' },
      { key: 'endpoint', example: 'https://minio.internal:9000', compatible: true },
    ],
  },
  {
    kind: 'kafka',
    direction: 'destination',
    transport: 'sink',
    auth: ['none', 'sasl_plain'],
    fields: [
      { key: 'brokers', required: true, example: 'broker-1:9092,broker-2:9092' },
      { key: 'topic', required: true, example: 'logs' },
    ],
  },
];

/** The config keys each way of authenticating fills in, secrets and all. */
export const AUTH_FIELDS: Record<AuthMode, readonly { key: string; secret: boolean }[]> = {
  access_key: [
    { key: 'access_key', secret: true },
    { key: 'secret_key', secret: true },
  ],
  temporary_key: [
    { key: 'access_key', secret: true },
    { key: 'secret_key', secret: true },
    { key: 'session_token', secret: true },
  ],
  node_role: [],
  sasl_plain: [
    { key: 'sasl_username', secret: false },
    { key: 'sasl_password', secret: true },
  ],
  none: [],
  push_token: [{ key: 'push_token', secret: true }],
};

/** What the API shows in place of a secret; sent back, it means "keep what is stored". */
export const SECRET_MASK = '***';

/** A push token shorter than this is refused by the backend. */
export const MIN_PUSH_TOKEN_LENGTH = 16;

export function isConnectorKind(kind: string): kind is ConnectorKind {
  return KINDS.some((spec) => spec.kind === kind);
}

export function specOf(kind: ConnectorKind): KindSpec {
  const spec = KINDS.find((item) => item.kind === kind);
  if (!spec) throw new Error(`unknown connector kind: ${kind}`);

  return spec;
}

/** The kinds a pipeline can deliver to. A source connector is not a destination. */
export function isDestinationKind(kind: string): boolean {
  return isConnectorKind(kind) && specOf(kind).direction === 'destination';
}

/** A connector being written: a new one, or a saved one opened to be changed. */
export interface ConnectorDraft {
  name: string;
  kind: ConnectorKind;
  enabled: boolean;
  auth: AuthMode;
  /** Every text setting by its config key, credentials included. A saved secret is empty here. */
  values: Record<string, string>;
  /** The bucket is addressed as a host name, as some S3-compatible stores require. */
  virtualHostedStyle: boolean;
  /** The S3-compatible settings are open. */
  compatible: boolean;
}

export function newDraft(kind: ConnectorKind): ConnectorDraft {
  const spec = specOf(kind);

  return {
    name: '',
    kind,
    enabled: true,
    auth: spec.auth[0] ?? 'none',
    values: {},
    virtualHostedStyle: false,
    compatible: false,
  };
}

const text = (value: unknown): string => (typeof value === 'string' ? value : '');

/** Whether a saved connector holds a value for this secret. */
export function hasStoredSecret(connector: Connector, key: string): boolean {
  return connector.secret_keys
    ? connector.secret_keys.includes(key)
    : // An older backend does not list them; a masked key is one that is there.
      text(connector.config_json[key]) === SECRET_MASK;
}

/**
 * Whether the backend keeps a stored secret when the mask is sent back. One
 * that does says so by listing `secret_keys`. Against one that does not, a
 * secret can only be saved by typing it again.
 */
export function keepsSecrets(connector: Connector): boolean {
  return Array.isArray(connector.secret_keys);
}

/** How a saved connector authenticates, read from what it has stored. */
export function authModeOf(connector: Connector): AuthMode {
  const kind = isConnectorKind(connector.kind) ? connector.kind : null;
  const modes = kind ? specOf(kind).auth : [];
  const has = (key: string) => hasStoredSecret(connector, key) || text(connector.config_json[key]).trim() !== '';

  if (modes.includes('push_token')) return 'push_token';
  if (modes.includes('temporary_key') && has('session_token')) return 'temporary_key';
  if (modes.includes('access_key') && has('access_key')) return 'access_key';
  if (modes.includes('sasl_plain') && has('sasl_username')) return 'sasl_plain';
  if (modes.includes('node_role')) return 'node_role';
  if (modes.includes('none')) return 'none';

  return modes[0] ?? 'none';
}

/** A saved connector as a draft. Its secrets are not known here, so they open empty. */
export function draftOf(connector: Connector): ConnectorDraft {
  const kind = isConnectorKind(connector.kind) ? connector.kind : 's3';
  const values: Record<string, string> = {};

  for (const [key, value] of Object.entries(connector.config_json)) {
    if (typeof value === 'string' && value !== SECRET_MASK) values[key] = value;
  }

  return {
    name: connector.name,
    kind,
    enabled: connector.enabled,
    auth: authModeOf(connector),
    values,
    virtualHostedStyle: connector.config_json.virtual_hosted_style === true,
    compatible: text(connector.config_json.endpoint).trim() !== '' || connector.config_json.virtual_hosted_style === true,
  };
}

export type DraftProblem =
  | { code: 'name_missing' }
  | { code: 'field_missing'; key: string }
  | { code: 'endpoint_invalid' }
  | { code: 'secret_missing'; key: string }
  | { code: 'push_token_short' };

/**
 * What keeps a draft from being saved, the first thing first. `saved` is the
 * connector being changed: a secret it already holds may be left empty, as
 * long as the backend keeps it and the way of authenticating is the same.
 */
export function draftProblem(draft: ConnectorDraft, saved?: Connector): DraftProblem | null {
  const spec = specOf(draft.kind);
  const value = (key: string) => (draft.values[key] ?? '').trim();

  if (!draft.name.trim()) return { code: 'name_missing' };

  for (const field of spec.fields) {
    if (field.required && !value(field.key)) return { code: 'field_missing', key: field.key };
  }
  if (draft.compatible && value('endpoint') && !/^https?:\/\//.test(value('endpoint'))) {
    return { code: 'endpoint_invalid' };
  }

  for (const field of AUTH_FIELDS[draft.auth]) {
    if (value(field.key)) continue;
    if (field.secret && saved && keepsStored(draft, saved, field.key)) continue;

    return { code: 'secret_missing', key: field.key };
  }
  if (draft.auth === 'push_token' && value('push_token') && value('push_token').length < MIN_PUSH_TOKEN_LENGTH) {
    return { code: 'push_token_short' };
  }

  return null;
}

/** Whether leaving this secret empty keeps the stored one. */
function keepsStored(draft: ConnectorDraft, saved: Connector, key: string): boolean {
  return (
    keepsSecrets(saved) &&
    saved.kind === draft.kind &&
    hasStoredSecret(saved, key) &&
    AUTH_FIELDS[authModeOf(saved)].some((field) => field.key === key)
  );
}

/**
 * The `config_json` a draft amounts to. Only what the chosen way of
 * authenticating uses is sent, so switching from an access key to the node
 * role really removes the key. A secret left empty on a saved connector goes
 * back as the mask, which the backend reads as "keep it".
 */
export function configOf(draft: ConnectorDraft, saved?: Connector): Record<string, unknown> {
  const spec = specOf(draft.kind);
  const config: Record<string, unknown> = {};
  const value = (key: string) => (draft.values[key] ?? '').trim();

  for (const field of spec.fields) {
    if (field.compatible && !draft.compatible) continue;
    if (value(field.key)) config[field.key] = value(field.key);
  }
  if (draft.kind === 's3' && draft.compatible && draft.virtualHostedStyle) {
    config.virtual_hosted_style = true;
  }

  for (const field of AUTH_FIELDS[draft.auth]) {
    if (value(field.key)) config[field.key] = value(field.key);
    else if (field.secret && saved && keepsStored(draft, saved, field.key)) config[field.key] = SECRET_MASK;
  }

  return config;
}

/** Where a connector reads from or delivers to, in a line. */
export function placeOf(connector: Connector): string {
  const value = (key: string) => text(connector.config_json[key]).trim();

  switch (connector.kind) {
    case 's3': {
      const path = `s3://${value('bucket')}${value('prefix') ? `/${value('prefix').replace(/^\/+/, '')}` : ''}`;
      const endpoint = value('endpoint').replace(/^https?:\/\//, '');
      return endpoint ? `${path} · ${endpoint}` : path;
    }
    case 'kafka':
      return [value('topic'), value('brokers')].filter(Boolean).join(' · ');
    case 'aws_cloudwatch_logs':
      return [value('log_group'), value('region')].filter(Boolean).join(' · ');
    default:
      return '';
  }
}

/** The stream a source connector writes what it receives to. */
export function targetStreamOf(connector: Connector): string {
  const named = text(connector.config_json.target_stream).trim();
  if (named) return named;

  // What the push receivers fall back to when none is named.
  switch (connector.kind) {
    case 'aws_kinesis_firehose':
      return 'kinesis';
    case 'cloudflare_logpush':
      return 'cloudflare';
    case 'heroku_drain':
      return 'heroku';
    default:
      return 'default';
  }
}
