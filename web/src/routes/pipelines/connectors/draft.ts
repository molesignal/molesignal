import type { TFunction } from 'i18next';

import type { Connector } from '@/api/connectors';

import {
  AUTH_FIELDS,
  BASE_KEY_FIELDS,
  MIN_HMAC_SECRET_LENGTH,
  MIN_PUSH_TOKEN_LENGTH,
  SECRET_MASK,
  assumesWithKey,
  authFieldsOf,
  authModeOf,
  hasStoredSecret,
  isConnectorKind,
  isKafkaKind,
  isS3Kind,
  keepsSecrets,
  specOf,
  type AuthField,
  type AuthMode,
  type ConnectorKind,
  type FieldSpec,
} from './kinds';

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
  /** The connection to the Kafka brokers is encrypted with TLS. */
  tls: boolean;
  /** A role is assumed with an access key rather than with the node's identity. */
  baseKey: boolean;
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
    tls: false,
    baseKey: false,
  };
}

/** The settings a draft shows: the S3-compatible ones while those are open, the TLS ones over TLS. */
export function shownFields(draft: ConnectorDraft): readonly FieldSpec[] {
  return specOf(draft.kind).fields.filter(
    (field) => (!field.compatible || draft.compatible) && (!field.tls || draft.tls),
  );
}

/** The values the draft's way of authenticating fills in. */
export function authFields(draft: ConnectorDraft): readonly AuthField[] {
  return draft.auth === 'assume_role' && draft.baseKey
    ? [...AUTH_FIELDS.assume_role, ...BASE_KEY_FIELDS]
    : AUTH_FIELDS[draft.auth];
}

/** Settings the server works out itself, which a draft does not carry. */
const DERIVED_KEYS = ['auth', 'urlOrigin'];

/** A saved connector as a draft. Its secrets are not known here, so they open empty. */
export function draftOf(connector: Connector): ConnectorDraft {
  const kind = isConnectorKind(connector.kind) ? connector.kind : 's3';
  const config = connector.configJson;
  const values: Record<string, string> = {};

  for (const [key, value] of Object.entries(config)) {
    if (typeof value === 'string' && value !== SECRET_MASK && !DERIVED_KEYS.includes(key)) values[key] = value;
  }

  const endpoint = typeof config.endpoint === 'string' ? config.endpoint.trim() : '';

  return {
    name: connector.name,
    kind,
    enabled: connector.enabled,
    auth: authModeOf(connector),
    values,
    virtualHostedStyle: config.virtualHostedStyle === true,
    compatible: endpoint !== '' || config.virtualHostedStyle === true,
    tls: config.tls === true || config.tls === 'true',
    baseKey: assumesWithKey(connector),
  };
}

/** Whether the saved connector holds this secret for what the draft does with it. */
export function storedFor(draft: ConnectorDraft, saved: Connector | undefined, key: string): boolean {
  if (!saved || saved.kind !== draft.kind || !hasStoredSecret(saved, key)) return false;

  // A secret setting of the kind, an HTTP destination's address, whatever the way of authenticating.
  if (specOf(draft.kind).fields.some((field) => field.key === key && field.secret)) return true;

  return authFieldsOf(saved).some((field) => field.key === key);
}

/** Whether leaving this secret empty keeps the stored one. */
export function keepsStored(draft: ConnectorDraft, saved: Connector | undefined, key: string): boolean {
  return saved !== undefined && keepsSecrets(saved) && storedFor(draft, saved, key);
}

export type DraftProblem =
  | { code: 'name_missing' }
  | { code: 'field_missing'; key: string }
  | { code: 'endpoint_invalid' }
  | { code: 'url_invalid' }
  | { code: 'secret_missing'; key: string }
  | { code: 'push_token_short' }
  | { code: 'role_arn_invalid' }
  | { code: 'tls_required' }
  | { code: 'pem_invalid'; key: string }
  | { code: 'hmac_secret_short' }
  | { code: 'header_name_invalid' }
  | { code: 'header_name_reserved' };

/** `arn:<partition>:iam::<12-digit account>:role/<path and name>`, as the backend reads it. */
const ROLE_ARN = /^arn:aws[a-z-]*:iam::\d{12}:role\/.+$/;

/** An HTTP header name: a token of RFC 9110. */
const HEADER_NAME = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;

/** Headers the delivery sets itself. */
const RESERVED_HEADERS = [
  'host',
  'content-length',
  'content-type',
  'content-encoding',
  'transfer-encoding',
  'connection',
  'x-molesignal-timestamp',
  'x-molesignal-sign',
];

/** PEM text whose `-----BEGIN` line names `label`: a certificate, or a private key. */
function isPem(text: string, label: string): boolean {
  return text.split('\n').some((line) => line.startsWith('-----BEGIN ') && line.trimEnd().endsWith(`${label}-----`));
}

/**
 * What keeps a draft from being saved, the first thing first. `saved` is the
 * connector being changed: a secret it already holds may be left empty, as
 * long as the backend keeps it and the way of authenticating still uses it.
 */
export function draftProblem(draft: ConnectorDraft, saved?: Connector): DraftProblem | null {
  if (!draft.name.trim()) return { code: 'name_missing' };

  return settingsProblem(draft, saved);
}

/** What keeps a draft's settings from working, its name aside: a connection is tested before it is named. */
export function settingsProblem(draft: ConnectorDraft, saved?: Connector): DraftProblem | null {
  const value = (key: string) => (draft.values[key] ?? '').trim();
  const kept = (key: string) => keepsStored(draft, saved, key);

  for (const field of shownFields(draft)) {
    if (field.required && !value(field.key) && !(field.secret && kept(field.key))) {
      return { code: 'field_missing', key: field.key };
    }
  }
  if (draft.compatible && value('endpoint') && !/^https?:\/\//.test(value('endpoint'))) {
    return { code: 'endpoint_invalid' };
  }
  if (draft.kind === 'http' && value('url') && !/^https?:\/\/[^/\s]+/.test(value('url'))) {
    return { code: 'url_invalid' };
  }
  if (draft.tls && value('tlsCaCert') && !isPem(value('tlsCaCert'), 'CERTIFICATE')) {
    return { code: 'pem_invalid', key: 'tlsCaCert' };
  }

  for (const field of authFields(draft)) {
    if (value(field.key)) continue;
    if (field.secret && kept(field.key)) continue;

    return { code: 'secret_missing', key: field.key };
  }

  return authProblem(draft, value);
}

function authProblem(draft: ConnectorDraft, value: (key: string) => string): DraftProblem | null {
  switch (draft.auth) {
    case 'assume_role':
    case 'web_identity':
      return ROLE_ARN.test(value('roleArn')) ? null : { code: 'role_arn_invalid' };
    case 'mtls':
      if (!draft.tls) return { code: 'tls_required' };
      if (value('tlsClientCert') && !isPem(value('tlsClientCert'), 'CERTIFICATE')) {
        return { code: 'pem_invalid', key: 'tlsClientCert' };
      }
      if (value('tlsClientKey') && !isPem(value('tlsClientKey'), 'PRIVATE KEY')) {
        return { code: 'pem_invalid', key: 'tlsClientKey' };
      }
      return null;
    case 'header':
      if (!HEADER_NAME.test(value('headerName'))) return { code: 'header_name_invalid' };
      return RESERVED_HEADERS.includes(value('headerName').toLowerCase()) ? { code: 'header_name_reserved' } : null;
    case 'hmac':
      return value('hmacSecret') && value('hmacSecret').length < MIN_HMAC_SECRET_LENGTH
        ? { code: 'hmac_secret_short' }
        : null;
    case 'push_token':
      return value('pushToken') && value('pushToken').length < MIN_PUSH_TOKEN_LENGTH
        ? { code: 'push_token_short' }
        : null;
    default:
      return null;
  }
}

/** A problem as the interface says it. */
export function problemText(problem: DraftProblem, t: TFunction<'pipelines'>): string {
  if (problem.code === 'field_missing' || problem.code === 'secret_missing' || problem.code === 'pem_invalid') {
    return t(`connectors.problems.${problem.code}`, { field: t(`connectors.fields.${problem.key}`) });
  }

  return t(`connectors.problems.${problem.code}`);
}

/**
 * The `config_json` a draft amounts to. Only what the chosen way of
 * authenticating uses is sent, so switching from an access key to the node's
 * identity really removes the key. A secret left empty on a saved connector
 * goes back as the mask, which the backend reads as "keep it".
 */
export function configOf(draft: ConnectorDraft, saved?: Connector): Record<string, unknown> {
  const spec = specOf(draft.kind);
  const config: Record<string, unknown> = {};
  const value = (key: string) => (draft.values[key] ?? '').trim();

  const put = (field: { key: string; secret?: boolean }) => {
    if (value(field.key)) config[field.key] = value(field.key);
    else if (field.secret && keepsStored(draft, saved, field.key)) config[field.key] = SECRET_MASK;
  };

  for (const field of shownFields(draft)) put(field);
  if (isS3Kind(draft.kind) && draft.compatible && draft.virtualHostedStyle) config.virtualHostedStyle = true;
  if (isKafkaKind(draft.kind) && draft.tls) config.tls = true;

  // A kind with more than one way of authenticating says which it takes.
  if (spec.auth.length > 1) config.auth = draft.auth;
  for (const field of authFields(draft)) put(field);

  return config;
}
