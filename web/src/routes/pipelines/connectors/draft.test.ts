import { describe, expect, it } from 'vitest';

import type { Connector } from '@/api/connectors';

import {
  configOf,
  draftOf,
  draftProblem,
  newDraft,
  settingsProblem,
  shownFields,
  type ConnectorDraft,
} from './draft';

function connector(kind: string, config: Record<string, unknown>, extra: Partial<Connector> = {}): Connector {
  return {
    id: 'c1',
    name: 'saved',
    kind,
    configJson: config,
    // A backend that keeps secrets says which it holds.
    secretKeys: Object.keys(config).filter((key) => config[key] === '***'),
    enabled: true,
    createdAt: 1,
    updatedAt: 1,
    ...extra,
  };
}

/** The same connector as an older backend returns it: masked, and no word on what it keeps. */
function fromOlderBackend(saved: Connector): Connector {
  const { secretKeys: _dropped, ...rest } = saved;
  return rest;
}

const draft = (kind: ConnectorDraft['kind'], patch: Partial<ConnectorDraft> = {}): ConnectorDraft => ({
  ...newDraft(kind),
  name: 'new',
  ...patch,
});

const ROLE = 'arn:aws:iam::123456789012:role/molesignal';
const CERT = '-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----';
const KEY = '-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----';

describe('a new connector', () => {
  it('starts on the usual way of authenticating for its kind', () => {
    expect(newDraft('s3').auth).toBe('access_key');
    expect(newDraft('kafka').auth).toBe('none');
    expect(newDraft('http').auth).toBe('none');
    expect(newDraft('cloudflare_logpush').auth).toBe('push_token');
  });

  it('needs a name, where it goes, and its credentials whole', () => {
    expect(draftProblem(draft('s3', { name: ' ' }))).toEqual({ code: 'name_missing' });
    expect(draftProblem(draft('s3'))).toEqual({ code: 'field_missing', key: 'bucket' });
    expect(draftProblem(draft('s3', { values: { bucket: 'b' } }))).toEqual({ code: 'secret_missing', key: 'accessKey' });
    expect(draftProblem(draft('s3', { values: { bucket: 'b', accessKey: 'AKIA' } }))).toEqual({
      code: 'secret_missing',
      key: 'secretKey',
    });
    expect(draftProblem(draft('s3', { values: { bucket: 'b', accessKey: 'AKIA', secretKey: 's' } }))).toBeNull();
    expect(
      draftProblem(draft('s3', { auth: 'temporary_key', values: { bucket: 'b', accessKey: 'ASIA', secretKey: 's' } })),
    ).toEqual({ code: 'secret_missing', key: 'sessionToken' });
    // The node's own identity asks for nothing more.
    expect(draftProblem(draft('s3', { auth: 'node_role', values: { bucket: 'b' } }))).toBeNull();
    expect(draftProblem(draft('kafka', { values: { brokers: 'b:9092' } }))).toEqual({ code: 'field_missing', key: 'topic' });
    expect(
      draftProblem(draft('kafka', { auth: 'sasl_plain', values: { brokers: 'b:9092', topic: 't', saslUsername: 'svc' } })),
    ).toEqual({ code: 'secret_missing', key: 'saslPassword' });
  });

  it('can be tested before it is named', () => {
    const unnamed = draft('s3', { name: '', auth: 'node_role', values: { bucket: 'b' } });

    expect(draftProblem(unnamed)).toEqual({ code: 'name_missing' });
    expect(settingsProblem(unnamed)).toBeNull();
  });

  it('wants an endpoint that is an address, and a token long enough to be a credential', () => {
    const compatible = { compatible: true, auth: 'node_role' as const };

    expect(draftProblem(draft('s3', { ...compatible, values: { bucket: 'b', endpoint: 'minio:9000' } }))).toEqual({
      code: 'endpoint_invalid',
    });
    expect(draftProblem(draft('s3', { ...compatible, values: { bucket: 'b', endpoint: 'http://minio:9000' } }))).toBeNull();
    expect(draftProblem(draft('heroku_drain'))).toEqual({ code: 'secret_missing', key: 'pushToken' });
    expect(draftProblem(draft('heroku_drain', { values: { pushToken: 'short' } }))).toEqual({ code: 'push_token_short' });
    expect(draftProblem(draft('heroku_drain', { values: { pushToken: '0123456789abcdef' } }))).toBeNull();
  });

  it('sends only what its way of authenticating uses, and says which way that is', () => {
    const values = { bucket: ' logs ', region: 'us-east-1', prefix: '', accessKey: 'AKIA', secretKey: 's', sessionToken: 't' };

    expect(configOf(draft('s3', { values }))).toEqual({
      bucket: 'logs',
      region: 'us-east-1',
      auth: 'access_key',
      accessKey: 'AKIA',
      secretKey: 's',
    });
    expect(configOf(draft('s3', { auth: 'temporary_key', values }))).toMatchObject({
      auth: 'temporary_key',
      sessionToken: 't',
    });
    // Switching to the node's identity really takes the key out.
    expect(configOf(draft('s3', { auth: 'node_role', values }))).toEqual({
      bucket: 'logs',
      region: 'us-east-1',
      auth: 'node_role',
    });
    // A push connector has only its token, and no word on it.
    expect(configOf(draft('heroku_drain', { values: { pushToken: '0123456789abcdef' } }))).toEqual({
      pushToken: '0123456789abcdef',
    });
  });

  it('keeps the S3-compatible settings to a connector that has them open', () => {
    const values = { bucket: 'b', endpoint: 'https://oss.example.com' };

    expect(configOf(draft('s3', { auth: 'node_role', values, virtualHostedStyle: true }))).toEqual({
      bucket: 'b',
      auth: 'node_role',
    });
    expect(configOf(draft('s3', { auth: 'node_role', values, compatible: true, virtualHostedStyle: true }))).toEqual({
      bucket: 'b',
      endpoint: 'https://oss.example.com',
      virtualHostedStyle: true,
      auth: 'node_role',
    });
  });
});

describe('an AWS role', () => {
  const base = { bucket: 'b', roleArn: ROLE };

  it('is named by its ARN', () => {
    expect(draftProblem(draft('s3', { auth: 'assume_role', values: { bucket: 'b' } }))).toEqual({
      code: 'secret_missing',
      key: 'roleArn',
    });
    expect(draftProblem(draft('s3', { auth: 'assume_role', values: { bucket: 'b', roleArn: 'molesignal' } }))).toEqual({
      code: 'role_arn_invalid',
    });
    expect(
      draftProblem(
        draft('aws_cloudwatch_logs', {
          auth: 'web_identity',
          values: { region: 'eu-west-1', logGroup: '/aws/fn', roleArn: 'arn:aws:iam::123456789012:user/me' },
        }),
      ),
    ).toEqual({ code: 'role_arn_invalid' });
    expect(draftProblem(draft('s3', { auth: 'assume_role', values: base }))).toBeNull();
  });

  it('is assumed as the node unless a key is asked for', () => {
    const values = { ...base, accessKey: 'AKIA', secretKey: 's' };

    expect(configOf(draft('s3', { auth: 'assume_role', values }))).toEqual({
      bucket: 'b',
      auth: 'assume_role',
      roleArn: ROLE,
    });
    expect(configOf(draft('s3', { auth: 'assume_role', baseKey: true, values }))).toEqual({
      bucket: 'b',
      auth: 'assume_role',
      roleArn: ROLE,
      accessKey: 'AKIA',
      secretKey: 's',
    });
    expect(draftProblem(draft('s3', { auth: 'assume_role', baseKey: true, values: base }))).toEqual({
      code: 'secret_missing',
      key: 'accessKey',
    });
  });

  it('assumed with a key opens with that key kept', () => {
    const saved = connector('s3', {
      bucket: 'b',
      auth: 'assume_role',
      roleArn: ROLE,
      accessKey: '***',
      secretKey: '***',
    });
    const opened = draftOf(saved);

    expect(opened.baseKey).toBe(true);
    expect(draftProblem(opened, saved)).toBeNull();
    expect(configOf(opened, saved)).toMatchObject({ accessKey: '***', secretKey: '***' });
    // Unticking the key really takes it out.
    expect(configOf({ ...opened, baseKey: false }, saved)).not.toHaveProperty('accessKey');
  });
});

describe('a Kafka destination', () => {
  const brokers = { brokers: 'b-1:9096', topic: 'logs' };

  it('shows the CA certificate only over TLS, and sends TLS as a switch', () => {
    expect(shownFields(draft('kafka')).map((field) => field.key)).toEqual(['brokers', 'topic']);
    expect(shownFields(draft('kafka', { tls: true })).map((field) => field.key)).toEqual(['brokers', 'topic', 'tlsCaCert']);
    expect(configOf(draft('kafka', { tls: true, values: { ...brokers, tlsCaCert: CERT } }))).toEqual({
      ...brokers,
      tlsCaCert: CERT,
      tls: true,
      auth: 'none',
    });
    // A CA typed before TLS was turned off is not sent.
    expect(configOf(draft('kafka', { values: { ...brokers, tlsCaCert: CERT } }))).toEqual({ ...brokers, auth: 'none' });
  });

  it('takes SCRAM with a user name and password', () => {
    const scram = draft('kafka', {
      auth: 'sasl_scram_sha_512',
      tls: true,
      values: { ...brokers, saslUsername: 'svc', saslPassword: 'pw' },
    });

    expect(draftProblem(scram)).toBeNull();
    expect(configOf(scram)).toEqual({
      ...brokers,
      tls: true,
      auth: 'sasl_scram_sha_512',
      saslUsername: 'svc',
      saslPassword: 'pw',
    });
  });

  it('presents a client certificate over TLS only, as PEM', () => {
    const values = { ...brokers, tlsClientCert: CERT, tlsClientKey: KEY };

    expect(draftProblem(draft('kafka', { auth: 'mtls', values }))).toEqual({ code: 'tls_required' });
    expect(draftProblem(draft('kafka', { auth: 'mtls', tls: true, values }))).toBeNull();
    expect(draftProblem(draft('kafka', { auth: 'mtls', tls: true, values: { ...values, tlsClientKey: CERT } }))).toEqual({
      code: 'pem_invalid',
      key: 'tlsClientKey',
    });
    expect(draftProblem(draft('kafka', { tls: true, values: { ...brokers, tlsCaCert: 'not pem' } }))).toEqual({
      code: 'pem_invalid',
      key: 'tlsCaCert',
    });
  });
});

describe('an HTTP destination', () => {
  const url = 'https://in.example.com/v1/logs';

  it('needs an http or https address', () => {
    expect(draftProblem(draft('http'))).toEqual({ code: 'field_missing', key: 'url' });
    expect(draftProblem(draft('http', { values: { url: 'in.example.com/logs' } }))).toEqual({ code: 'url_invalid' });
    expect(draftProblem(draft('http', { values: { url } }))).toBeNull();
  });

  it('sends the method and format chosen, and the credential of its way', () => {
    expect(configOf(draft('http', { values: { url, method: 'put', format: 'json' } }))).toEqual({
      url,
      method: 'put',
      format: 'json',
      auth: 'none',
    });
    expect(
      configOf(draft('http', { auth: 'header', values: { url, headerName: 'DD-API-KEY', headerValue: 'k' } })),
    ).toEqual({ url, auth: 'header', headerName: 'DD-API-KEY', headerValue: 'k' });
  });

  it('refuses a header the delivery sets itself, and a short signing secret', () => {
    const header = (name: string) =>
      draftProblem(draft('http', { auth: 'header', values: { url, headerName: name, headerValue: 'v' } }));

    expect(header('X-Api-Key')).toBeNull();
    expect(header('Content-Type')).toEqual({ code: 'header_name_reserved' });
    expect(header('X Api Key')).toEqual({ code: 'header_name_invalid' });
    expect(draftProblem(draft('http', { auth: 'hmac', values: { url, hmacSecret: 'short' } }))).toEqual({
      code: 'hmac_secret_short',
    });
    expect(draftProblem(draft('http', { auth: 'hmac', values: { url, hmacSecret: '0123456789abcdef' } }))).toBeNull();
  });

  it('keeps its saved address, which is a secret, when it is left empty', () => {
    const saved = connector('http', {
      url: '***',
      urlOrigin: 'https://in.example.com',
      auth: 'bearer',
      bearerToken: '***',
    });
    const opened = draftOf(saved);

    expect(opened.values).toEqual({});
    expect(draftProblem(opened, saved)).toBeNull();
    expect(configOf(opened, saved)).toEqual({ url: '***', auth: 'bearer', bearerToken: '***' });
  });
});

describe('a saved connector, changed', () => {
  const saved = connector('s3', {
    bucket: 'logs',
    endpoint: 'https://oss.example.com',
    virtualHostedStyle: true,
    auth: 'access_key',
    accessKey: '***',
    secretKey: '***',
  });

  it('opens with what is known of it, its secrets left out', () => {
    expect(draftOf(saved)).toEqual({
      name: 'saved',
      kind: 's3',
      enabled: true,
      auth: 'access_key',
      values: { bucket: 'logs', endpoint: 'https://oss.example.com' },
      virtualHostedStyle: true,
      compatible: true,
      tls: false,
      baseKey: false,
    });
  });

  it('sends the mask back for a secret left alone, which keeps it', () => {
    const edited = { ...draftOf(saved), values: { ...draftOf(saved).values, bucket: 'logs-2' } };

    expect(draftProblem(edited, saved)).toBeNull();
    expect(configOf(edited, saved)).toEqual({
      bucket: 'logs-2',
      endpoint: 'https://oss.example.com',
      virtualHostedStyle: true,
      auth: 'access_key',
      accessKey: '***',
      secretKey: '***',
    });
  });

  it('sends a secret that was typed, and asks for one the saved connector never had', () => {
    const rotated = { ...draftOf(saved), values: { ...draftOf(saved).values, secretKey: 'new-secret' } };
    expect(configOf(rotated, saved)).toMatchObject({ accessKey: '***', secretKey: 'new-secret' });

    const temporary = { ...draftOf(saved), auth: 'temporary_key' as const };
    expect(draftProblem(temporary, saved)).toEqual({ code: 'secret_missing', key: 'sessionToken' });
  });

  it('asks for every secret again where the backend would not keep them', () => {
    const older = fromOlderBackend(saved);
    const untouched = draftOf(older);

    expect(draftProblem(untouched, older)).toEqual({ code: 'secret_missing', key: 'accessKey' });
    // Nothing masked is ever sent to a backend that would save the mask.
    expect(Object.values(configOf(untouched, older))).not.toContain('***');
  });

  it('does not carry one kind of key into another way of authenticating', () => {
    const kafka = connector('kafka', { brokers: 'b:9092', topic: 't', saslUsername: 'svc', saslPassword: '***' });

    expect(configOf({ ...draftOf(kafka), auth: 'none' }, kafka)).toEqual({ brokers: 'b:9092', topic: 't', auth: 'none' });
    // Another SASL mechanism takes the same user, and keeps the password.
    expect(configOf({ ...draftOf(kafka), auth: 'sasl_scram_sha_256' }, kafka)).toEqual({
      brokers: 'b:9092',
      topic: 't',
      auth: 'sasl_scram_sha_256',
      saslUsername: 'svc',
      saslPassword: '***',
    });
  });

  it('saves where a Kafka or S3 source starts, its TLS and its compatible store', () => {
    const kafka = newDraft('kafka_source');
    kafka.tls = true;
    kafka.values = { brokers: 'b:9093', topic: 'logs', targetStream: 'app', startFrom: 'earliest' };

    expect(settingsProblem(kafka)).toBeNull();
    expect(configOf(kafka)).toEqual({
      brokers: 'b:9093',
      topic: 'logs',
      targetStream: 'app',
      startFrom: 'earliest',
      tls: true,
      auth: 'none',
    });

    const s3 = newDraft('s3_source');
    s3.compatible = true;
    s3.virtualHostedStyle = true;
    s3.auth = 'node_role';
    s3.values = { bucket: 'logs', endpoint: 'https://minio:9000' };

    expect(settingsProblem(s3)).toBeNull();
    expect(configOf(s3)).toEqual({
      bucket: 'logs',
      endpoint: 'https://minio:9000',
      virtualHostedStyle: true,
      auth: 'node_role',
    });
  });
});
