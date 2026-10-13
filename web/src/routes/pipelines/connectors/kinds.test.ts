import { describe, expect, it } from 'vitest';

import type { Connector } from '@/api/connectors';

import {
  authModeOf,
  configOf,
  draftOf,
  draftProblem,
  hasStoredSecret,
  isDestinationKind,
  keepsSecrets,
  KINDS,
  newDraft,
  placeOf,
  targetStreamOf,
  type ConnectorDraft,
} from './kinds';

function connector(kind: string, config: Record<string, unknown>, extra: Partial<Connector> = {}): Connector {
  return {
    id: 'c1',
    name: 'saved',
    kind,
    config_json: config,
    // A backend that keeps secrets says which it holds.
    secret_keys: Object.keys(config).filter((key) => config[key] === '***'),
    enabled: true,
    created_at_micros: 1,
    updated_at_micros: 1,
    ...extra,
  };
}

/** The same connector as an older backend returns it: masked, and no word on what it keeps. */
function fromOlderBackend(saved: Connector): Connector {
  const { secret_keys: _dropped, ...rest } = saved;
  return rest;
}

const draft = (kind: ConnectorDraft['kind'], patch: Partial<ConnectorDraft> = {}): ConnectorDraft => ({
  ...newDraft(kind),
  name: 'new',
  ...patch,
});

describe('connector kinds', () => {
  it('tells what brings data in from what pipelines deliver to', () => {
    expect(KINDS.filter((spec) => spec.direction === 'destination').map((spec) => spec.kind)).toEqual(['s3', 'kafka']);
    expect(isDestinationKind('s3')).toBe(true);
    expect(isDestinationKind('aws_cloudwatch_logs')).toBe(false);
    expect(isDestinationKind('webhook')).toBe(false);
  });

  it('starts a new connector on the usual way of authenticating for its kind', () => {
    expect(newDraft('s3').auth).toBe('access_key');
    expect(newDraft('kafka').auth).toBe('none');
    expect(newDraft('cloudflare_logpush').auth).toBe('push_token');
  });
});

describe('how a saved connector authenticates', () => {
  it('is read from what it has stored', () => {
    expect(authModeOf(connector('s3', { bucket: 'b', access_key: '***', secret_key: '***' }))).toBe('access_key');
    expect(
      authModeOf(connector('s3', { bucket: 'b', access_key: '***', secret_key: '***', session_token: '***' })),
    ).toBe('temporary_key');
    expect(authModeOf(connector('s3', { bucket: 'b' }))).toBe('node_role');
    expect(authModeOf(connector('kafka', { brokers: 'b:9092', topic: 't' }))).toBe('none');
    expect(
      authModeOf(connector('kafka', { brokers: 'b:9092', topic: 't', sasl_username: 'svc', sasl_password: '***' })),
    ).toBe('sasl_plain');
    expect(authModeOf(connector('heroku_drain', {}))).toBe('push_token');
  });

  it('knows a stored secret by the list a newer backend sends, or by its mask', () => {
    const saved = connector('s3', { bucket: 'b', access_key: '***', secret_key: '***' });

    expect(keepsSecrets(saved)).toBe(true);
    expect(hasStoredSecret(saved, 'secret_key')).toBe(true);
    expect(hasStoredSecret(saved, 'session_token')).toBe(false);

    const older = fromOlderBackend(saved);
    expect(keepsSecrets(older)).toBe(false);
    expect(hasStoredSecret(older, 'secret_key')).toBe(true);
    expect(authModeOf(older)).toBe('access_key');
  });
});

describe('a new connector', () => {
  it('needs a name, where it goes, and its credentials whole', () => {
    expect(draftProblem(draft('s3', { name: ' ' }))).toEqual({ code: 'name_missing' });
    expect(draftProblem(draft('s3'))).toEqual({ code: 'field_missing', key: 'bucket' });
    expect(draftProblem(draft('s3', { values: { bucket: 'b' } }))).toEqual({ code: 'secret_missing', key: 'access_key' });
    expect(draftProblem(draft('s3', { values: { bucket: 'b', access_key: 'AKIA' } }))).toEqual({
      code: 'secret_missing',
      key: 'secret_key',
    });
    expect(draftProblem(draft('s3', { values: { bucket: 'b', access_key: 'AKIA', secret_key: 's' } }))).toBeNull();
    expect(
      draftProblem(draft('s3', { auth: 'temporary_key', values: { bucket: 'b', access_key: 'ASIA', secret_key: 's' } })),
    ).toEqual({ code: 'secret_missing', key: 'session_token' });
    // The node's own role asks for nothing more.
    expect(draftProblem(draft('s3', { auth: 'node_role', values: { bucket: 'b' } }))).toBeNull();
    expect(draftProblem(draft('kafka', { values: { brokers: 'b:9092' } }))).toEqual({ code: 'field_missing', key: 'topic' });
    expect(
      draftProblem(draft('kafka', { auth: 'sasl_plain', values: { brokers: 'b:9092', topic: 't', sasl_username: 'svc' } })),
    ).toEqual({ code: 'secret_missing', key: 'sasl_password' });
  });

  it('wants an endpoint that is an address, and a token long enough to be a credential', () => {
    const compatible = { compatible: true, auth: 'node_role' as const };

    expect(draftProblem(draft('s3', { ...compatible, values: { bucket: 'b', endpoint: 'minio:9000' } }))).toEqual({
      code: 'endpoint_invalid',
    });
    expect(draftProblem(draft('s3', { ...compatible, values: { bucket: 'b', endpoint: 'http://minio:9000' } }))).toBeNull();
    expect(draftProblem(draft('heroku_drain'))).toEqual({ code: 'secret_missing', key: 'push_token' });
    expect(draftProblem(draft('heroku_drain', { values: { push_token: 'short' } }))).toEqual({ code: 'push_token_short' });
    expect(draftProblem(draft('heroku_drain', { values: { push_token: '0123456789abcdef' } }))).toBeNull();
  });

  it('sends only what its way of authenticating uses', () => {
    const values = { bucket: ' logs ', region: 'us-east-1', prefix: '', access_key: 'AKIA', secret_key: 's', session_token: 't' };

    expect(configOf(draft('s3', { values }))).toEqual({
      bucket: 'logs',
      region: 'us-east-1',
      access_key: 'AKIA',
      secret_key: 's',
    });
    expect(configOf(draft('s3', { auth: 'temporary_key', values }))).toMatchObject({ session_token: 't' });
    // Switching to the node's role really takes the key out.
    expect(configOf(draft('s3', { auth: 'node_role', values }))).toEqual({ bucket: 'logs', region: 'us-east-1' });
  });

  it('keeps the S3-compatible settings to a connector that has them open', () => {
    const values = { bucket: 'b', endpoint: 'https://oss.example.com' };

    expect(configOf(draft('s3', { auth: 'node_role', values, virtualHostedStyle: true }))).toEqual({ bucket: 'b' });
    expect(configOf(draft('s3', { auth: 'node_role', values, compatible: true, virtualHostedStyle: true }))).toEqual({
      bucket: 'b',
      endpoint: 'https://oss.example.com',
      virtual_hosted_style: true,
    });
  });
});

describe('a saved connector, changed', () => {
  const saved = connector('s3', {
    bucket: 'logs',
    endpoint: 'https://oss.example.com',
    virtual_hosted_style: true,
    access_key: '***',
    secret_key: '***',
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
    });
  });

  it('sends the mask back for a secret left alone, which keeps it', () => {
    const edited = { ...draftOf(saved), values: { ...draftOf(saved).values, bucket: 'logs-2' } };

    expect(draftProblem(edited, saved)).toBeNull();
    expect(configOf(edited, saved)).toEqual({
      bucket: 'logs-2',
      endpoint: 'https://oss.example.com',
      virtual_hosted_style: true,
      access_key: '***',
      secret_key: '***',
    });
  });

  it('sends a secret that was typed, and asks for one the saved connector never had', () => {
    const rotated = { ...draftOf(saved), values: { ...draftOf(saved).values, secret_key: 'new-secret' } };
    expect(configOf(rotated, saved)).toMatchObject({ access_key: '***', secret_key: 'new-secret' });

    const temporary = { ...draftOf(saved), auth: 'temporary_key' as const };
    expect(draftProblem(temporary, saved)).toEqual({ code: 'secret_missing', key: 'session_token' });
  });

  it('asks for every secret again where the backend would not keep them', () => {
    const older = fromOlderBackend(saved);
    const untouched = draftOf(older);

    expect(draftProblem(untouched, older)).toEqual({ code: 'secret_missing', key: 'access_key' });
    // Nothing masked is ever sent to a backend that would save the mask.
    expect(Object.values(configOf(untouched, older))).not.toContain('***');
  });

  it('does not carry one kind of key into another way of authenticating', () => {
    const kafka = connector('kafka', { brokers: 'b:9092', topic: 't', sasl_username: 'svc', sasl_password: '***' });

    expect(configOf({ ...draftOf(kafka), auth: 'none' }, kafka)).toEqual({ brokers: 'b:9092', topic: 't' });
  });
});

describe('a connector in a line', () => {
  it('says where it delivers or what it reads', () => {
    expect(placeOf(connector('s3', { bucket: 'acme', prefix: '/out/' }))).toBe('s3://acme/out/');
    expect(placeOf(connector('s3', { bucket: 'acme', endpoint: 'https://minio.internal:9000' }))).toBe(
      's3://acme · minio.internal:9000',
    );
    expect(placeOf(connector('kafka', { brokers: 'b1:9092,b2:9092', topic: 'logs' }))).toBe('logs · b1:9092,b2:9092');
    expect(placeOf(connector('aws_cloudwatch_logs', { region: 'us-east-1', log_group: '/aws/fn' }))).toBe(
      '/aws/fn · us-east-1',
    );
  });

  it('names the stream a source writes to, as the receiver falls back when none is set', () => {
    expect(targetStreamOf(connector('cloudflare_logpush', { target_stream: ' edge ' }))).toBe('edge');
    expect(targetStreamOf(connector('cloudflare_logpush', {}))).toBe('cloudflare');
    expect(targetStreamOf(connector('aws_kinesis_firehose', {}))).toBe('kinesis');
    expect(targetStreamOf(connector('heroku_drain', {}))).toBe('heroku');
    expect(targetStreamOf(connector('aws_cloudwatch_logs', {}))).toBe('default');
  });
});
