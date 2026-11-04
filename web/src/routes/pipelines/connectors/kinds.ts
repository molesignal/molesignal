import type { Connector } from '@/api/connectors';

/**
 * The connector kinds the backend knows (`infra/connectors/mod.rs`) and what
 * each needs to work: where it reads from or delivers to, and how it proves
 * who it is. The keys are the ones the backend reads from `config_json`
 * (`infra/connectors/config/`); nothing is offered here that it does not do.
 */
export type ConnectorKind =
  | 'aws_cloudwatch_logs'
  | 'aws_kinesis_firehose'
  | 'cloudflare_logpush'
  | 'heroku_drain'
  | 's3'
  | 'kafka'
  | 'http'
  | 'kafka_source'
  | 's3_source';

/** Whether a connector brings data in, or carries what pipelines write out. */
export type Direction = 'source' | 'destination';

/**
 * How the data moves: fetched by MoleSignal on a timer, sent to it by the
 * other service, or delivered by a pipeline.
 */
export type Transport = 'pull' | 'push' | 'sink';

/**
 * How a connector authenticates, saved as `config_json.auth`.
 *
 * AWS (S3, CloudWatch Logs):
 *  - `access_key`: a long-lived key of an IAM user (id and secret);
 *  - `temporary_key`: a short-lived one, only valid with its session token;
 *  - `node_role`: no key at all, the identity of the node MoleSignal runs on;
 *  - `assume_role`: a role assumed through STS, with the organization's external ID;
 *  - `web_identity`: a role assumed with the node's web identity token (IRSA).
 *
 * Kafka: `none`, SASL (`sasl_plain`, `sasl_scram_sha_256`, `sasl_scram_sha_512`),
 * or a client certificate over TLS (`mtls`).
 *
 * HTTP: `none`, a `bearer` token, `basic` user name and password, a key in a
 * `header`, or an `hmac` signature of the body.
 *
 * A push connector's `pushToken` is what the other service presents.
 */
export type AuthMode =
  | 'access_key'
  | 'temporary_key'
  | 'node_role'
  | 'assume_role'
  | 'web_identity'
  | 'none'
  | 'sasl_plain'
  | 'sasl_scram_sha_256'
  | 'sasl_scram_sha_512'
  | 'mtls'
  | 'bearer'
  | 'basic'
  | 'header'
  | 'hmac'
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
  /** Belongs with the TLS settings, shown only while the connection is encrypted. */
  tls?: boolean;
  /** Kept like a credential: never read back, and a saved one stays when left empty. */
  secret?: boolean;
  /** Several lines, such as PEM text. */
  multiline?: boolean;
  /** One of these values, the first when none is chosen; each is worded as `connectors.choices.<key>.<value>`. */
  choices?: readonly string[];
}

/** A value a way of authenticating fills in. */
export interface AuthField {
  key: string;
  secret: boolean;
  multiline?: boolean;
  example?: string;
}

const TARGET_STREAM: FieldSpec = { key: 'targetStream', stream: true, example: 'default' };

const AWS_AUTH: readonly AuthMode[] = ['access_key', 'temporary_key', 'node_role', 'assume_role', 'web_identity'];

const START_FROM: FieldSpec = { key: 'startFrom', choices: ['latest', 'earliest'] };

const KAFKA_AUTH: readonly AuthMode[] = ['none', 'sasl_plain', 'sasl_scram_sha_256', 'sasl_scram_sha_512', 'mtls'];

export const KINDS: readonly KindSpec[] = [
  {
    kind: 'aws_cloudwatch_logs',
    direction: 'source',
    transport: 'pull',
    auth: AWS_AUTH,
    fields: [
      { key: 'region', required: true, example: 'us-east-1' },
      { key: 'logGroup', required: true, example: '/aws/lambda/checkout' },
      TARGET_STREAM,
    ],
  },
  {
    kind: 'kafka_source',
    direction: 'source',
    transport: 'pull',
    auth: KAFKA_AUTH,
    fields: [
      { key: 'brokers', required: true, example: 'broker-1:9092,broker-2:9092' },
      { key: 'topic', required: true, example: 'logs' },
      TARGET_STREAM,
      START_FROM,
      { key: 'tlsCaCert', tls: true, multiline: true, example: '-----BEGIN CERTIFICATE-----' },
    ],
  },
  {
    kind: 's3_source',
    direction: 'source',
    transport: 'pull',
    auth: AWS_AUTH,
    fields: [
      { key: 'bucket', required: true, example: 'acme-logs' },
      { key: 'region', example: 'us-east-1' },
      { key: 'prefix', example: 'AWSLogs/123456789012/' },
      TARGET_STREAM,
      START_FROM,
      { key: 'endpoint', example: 'https://minio.internal:9000', compatible: true },
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
    auth: AWS_AUTH,
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
    auth: KAFKA_AUTH,
    fields: [
      { key: 'brokers', required: true, example: 'broker-1:9092,broker-2:9092' },
      { key: 'topic', required: true, example: 'logs' },
      { key: 'tlsCaCert', tls: true, multiline: true, example: '-----BEGIN CERTIFICATE-----' },
    ],
  },
  {
    kind: 'http',
    direction: 'destination',
    transport: 'sink',
    auth: ['none', 'bearer', 'basic', 'header', 'hmac'],
    fields: [
      { key: 'url', required: true, secret: true, example: 'https://logs.example.com/ingest' },
      { key: 'method', choices: ['post', 'put'] },
      { key: 'format', choices: ['ndjson', 'json'] },
    ],
  },
];

const ACCESS_KEY: readonly AuthField[] = [
  { key: 'accessKey', secret: true },
  { key: 'secretKey', secret: true },
];

const SASL: readonly AuthField[] = [
  { key: 'saslUsername', secret: false },
  { key: 'saslPassword', secret: true },
];

const ROLE_ARN: AuthField = {
  key: 'roleArn',
  secret: false,
  example: 'arn:aws:iam::123456789012:role/molesignal',
};

/** The config keys each way of authenticating fills in, secrets and all. */
export const AUTH_FIELDS: Record<AuthMode, readonly AuthField[]> = {
  access_key: ACCESS_KEY,
  temporary_key: [...ACCESS_KEY, { key: 'sessionToken', secret: true }],
  node_role: [],
  assume_role: [ROLE_ARN],
  web_identity: [ROLE_ARN],
  none: [],
  sasl_plain: SASL,
  sasl_scram_sha_256: SASL,
  sasl_scram_sha_512: SASL,
  mtls: [
    { key: 'tlsClientCert', secret: false, multiline: true, example: '-----BEGIN CERTIFICATE-----' },
    { key: 'tlsClientKey', secret: true, multiline: true, example: '-----BEGIN PRIVATE KEY-----' },
  ],
  bearer: [{ key: 'bearerToken', secret: true }],
  basic: [
    { key: 'basicUsername', secret: false },
    { key: 'basicPassword', secret: true },
  ],
  header: [
    { key: 'headerName', secret: false, example: 'X-API-Key' },
    { key: 'headerValue', secret: true },
  ],
  hmac: [{ key: 'hmacSecret', secret: true }],
  push_token: [{ key: 'pushToken', secret: true }],
};

/**
 * The access key a role may be assumed with instead of the node's identity,
 * for a deployment that has none.
 */
export const BASE_KEY_FIELDS: readonly AuthField[] = ACCESS_KEY;

/** What the API shows in place of a secret; sent back, it means "keep what is stored". */
export const SECRET_MASK = '***';

/** A push token shorter than this is refused by the backend. */
export const MIN_PUSH_TOKEN_LENGTH = 16;

/** A shared signing secret shorter than this is refused by the backend. */
export const MIN_HMAC_SECRET_LENGTH = 16;

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

/** Whether a kind is reached over AWS, with AWS's ways of authenticating. */
export function isAwsKind(kind: ConnectorKind): boolean {
  return kind === 's3' || kind === 's3_source' || kind === 'aws_cloudwatch_logs';
}

/** Whether a kind talks to Kafka brokers. */
export function isKafkaKind(kind: ConnectorKind): boolean {
  return kind === 'kafka' || kind === 'kafka_source';
}

/** Whether a kind talks to S3 or a store like it. */
export function isS3Kind(kind: ConnectorKind): boolean {
  return kind === 's3' || kind === 's3_source';
}

const text = (value: unknown): string => (typeof value === 'string' ? value : '');

/** Whether a saved connector holds a value for this secret. */
export function hasStoredSecret(connector: Connector, key: string): boolean {
  return connector.secretKeys
    ? connector.secretKeys.includes(key)
    : // An older backend does not list them; a masked key is one that is there.
      text(connector.configJson[key]) === SECRET_MASK;
}

/**
 * Whether the backend keeps a stored secret when the mask is sent back. One
 * that does says so by listing `secret_keys`. Against one that does not, a
 * secret can only be saved by typing it again.
 */
export function keepsSecrets(connector: Connector): boolean {
  return Array.isArray(connector.secretKeys);
}

/**
 * How a saved connector authenticates: what it says under `auth`, or, for one
 * saved before `auth` was, what it has stored.
 */
export function authModeOf(connector: Connector): AuthMode {
  const kind = isConnectorKind(connector.kind) ? connector.kind : null;
  const modes = kind ? specOf(kind).auth : [];
  const named = text(connector.configJson.auth) as AuthMode;
  const has = (key: string) => hasStoredSecret(connector, key) || text(connector.configJson[key]).trim() !== '';

  if (modes.includes(named)) return named;
  if (modes.includes('push_token')) return 'push_token';
  if (modes.includes('temporary_key') && has('sessionToken')) return 'temporary_key';
  if (modes.includes('access_key') && has('accessKey')) return 'access_key';
  if (modes.includes('assume_role') && has('roleArn')) return 'assume_role';
  if (modes.includes('sasl_plain') && has('saslUsername')) return 'sasl_plain';
  if (modes.includes('node_role')) return 'node_role';
  if (modes.includes('none')) return 'none';

  return modes[0] ?? 'none';
}

/** Whether a role is assumed with a stored access key rather than the node's identity. */
export function assumesWithKey(connector: Connector): boolean {
  return authModeOf(connector) === 'assume_role' && hasStoredSecret(connector, 'accessKey');
}

/** The fields a saved connector's way of authenticating holds values for. */
export function authFieldsOf(connector: Connector): readonly AuthField[] {
  const mode = authModeOf(connector);

  return assumesWithKey(connector) ? [...AUTH_FIELDS[mode], ...BASE_KEY_FIELDS] : AUTH_FIELDS[mode];
}

/** Where a connector reads from or delivers to, in a line. */
export function placeOf(connector: Connector): string {
  const value = (key: string) => text(connector.configJson[key]).trim();

  switch (connector.kind) {
    case 's3':
    case 's3_source': {
      const path = `s3://${value('bucket')}${value('prefix') ? `/${value('prefix').replace(/^\/+/, '')}` : ''}`;
      const endpoint = value('endpoint').replace(/^https?:\/\//, '');
      return endpoint ? `${path} · ${endpoint}` : path;
    }
    case 'kafka':
    case 'kafka_source':
      return [value('topic'), value('brokers')].filter(Boolean).join(' · ');
    case 'http':
      // The address itself is kept as a secret; the server says where it points.
      return value('urlOrigin');
    case 'aws_cloudwatch_logs':
      return [value('logGroup'), value('region')].filter(Boolean).join(' · ');
    default:
      return '';
  }
}

/** The stream a source connector writes what it receives to. */
export function targetStreamOf(connector: Connector): string {
  const named = text(connector.configJson.targetStream).trim();
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
