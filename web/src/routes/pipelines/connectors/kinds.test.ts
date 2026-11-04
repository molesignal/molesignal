import { describe, expect, it } from 'vitest';

import type { Connector } from '@/api/connectors';

import {
  assumesWithKey,
  authFieldsOf,
  authModeOf,
  hasStoredSecret,
  isDestinationKind,
  keepsSecrets,
  KINDS,
  placeOf,
  targetStreamOf,
} from './kinds';

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

describe('connector kinds', () => {
  it('tells what brings data in from what pipelines deliver to', () => {
    expect(KINDS.filter((spec) => spec.direction === 'destination').map((spec) => spec.kind)).toEqual([
      's3',
      'kafka',
      'http',
    ]);
    expect(isDestinationKind('s3')).toBe(true);
    expect(isDestinationKind('http')).toBe(true);
    expect(isDestinationKind('aws_cloudwatch_logs')).toBe(false);
    expect(isDestinationKind('webhook')).toBe(false);
  });

  it('offers each kind only the ways its backend takes', () => {
    const auth = (kind: string) => KINDS.find((spec) => spec.kind === kind)?.auth;
    const aws = ['access_key', 'temporary_key', 'node_role', 'assume_role', 'web_identity'];

    expect(auth('s3')).toEqual(aws);
    expect(auth('aws_cloudwatch_logs')).toEqual(aws);
    expect(auth('kafka')).toEqual(['none', 'sasl_plain', 'sasl_scram_sha_256', 'sasl_scram_sha_512', 'mtls']);
    expect(auth('http')).toEqual(['none', 'bearer', 'basic', 'header', 'hmac']);
    expect(auth('heroku_drain')).toEqual(['push_token']);
    expect(auth('kafka_source')).toEqual(auth('kafka'));
    expect(auth('s3_source')).toEqual(aws);
  });

  it('counts a topic or a bucket to read as a source, not as somewhere to deliver', () => {
    expect(isDestinationKind('kafka_source')).toBe(false);
    expect(isDestinationKind('s3_source')).toBe(false);
    expect(KINDS.find((spec) => spec.kind === 'kafka_source')?.transport).toBe('pull');
    expect(KINDS.find((spec) => spec.kind === 's3_source')?.transport).toBe('pull');
  });
});

describe('how a saved connector authenticates', () => {
  it('is what it says under auth', () => {
    expect(authModeOf(connector('s3', { bucket: 'b', auth: 'web_identity', roleArn: 'arn:aws:iam::1:role/r' }))).toBe(
      'web_identity',
    );
    expect(authModeOf(connector('kafka', { brokers: 'b', topic: 't', auth: 'mtls', tls: true }))).toBe('mtls');
    expect(authModeOf(connector('http', { url: '***', auth: 'hmac', hmacSecret: '***' }))).toBe('hmac');
    // A way another kind takes is not taken for this one.
    expect(authModeOf(connector('s3', { bucket: 'b', auth: 'sasl_plain' }))).toBe('node_role');
  });

  it('is read from what it has stored when it predates auth', () => {
    expect(authModeOf(connector('s3', { bucket: 'b', accessKey: '***', secretKey: '***' }))).toBe('access_key');
    expect(
      authModeOf(connector('s3', { bucket: 'b', accessKey: '***', secretKey: '***', sessionToken: '***' })),
    ).toBe('temporary_key');
    expect(authModeOf(connector('s3', { bucket: 'b' }))).toBe('node_role');
    expect(authModeOf(connector('kafka', { brokers: 'b:9092', topic: 't' }))).toBe('none');
    expect(
      authModeOf(connector('kafka', { brokers: 'b:9092', topic: 't', saslUsername: 'svc', saslPassword: '***' })),
    ).toBe('sasl_plain');
    expect(authModeOf(connector('heroku_drain', {}))).toBe('push_token');
    expect(authModeOf(connector('http', { url: '***' }))).toBe('none');
  });

  it('knows a stored secret by the list a newer backend sends, or by its mask', () => {
    const saved = connector('s3', { bucket: 'b', accessKey: '***', secretKey: '***' });

    expect(keepsSecrets(saved)).toBe(true);
    expect(hasStoredSecret(saved, 'secretKey')).toBe(true);
    expect(hasStoredSecret(saved, 'sessionToken')).toBe(false);

    const older = fromOlderBackend(saved);
    expect(keepsSecrets(older)).toBe(false);
    expect(hasStoredSecret(older, 'secretKey')).toBe(true);
    expect(authModeOf(older)).toBe('access_key');
  });

  it('counts the key a role is assumed with among its credentials', () => {
    const role = 'arn:aws:iam::123456789012:role/delivery';
    const withKey = connector('s3', {
      bucket: 'b',
      auth: 'assume_role',
      roleArn: role,
      accessKey: '***',
      secretKey: '***',
    });
    const asNode = connector('s3', { bucket: 'b', auth: 'assume_role', roleArn: role });

    expect(assumesWithKey(withKey)).toBe(true);
    expect(authFieldsOf(withKey).map((field) => field.key)).toEqual(['roleArn', 'accessKey', 'secretKey']);
    expect(assumesWithKey(asNode)).toBe(false);
    expect(authFieldsOf(asNode).map((field) => field.key)).toEqual(['roleArn']);
  });
});

describe('a connector in a line', () => {
  it('says where it delivers or what it reads', () => {
    expect(placeOf(connector('s3', { bucket: 'acme', prefix: '/out/' }))).toBe('s3://acme/out/');
    expect(placeOf(connector('s3', { bucket: 'acme', endpoint: 'https://minio.internal:9000' }))).toBe(
      's3://acme · minio.internal:9000',
    );
    expect(placeOf(connector('kafka', { brokers: 'b1:9092,b2:9092', topic: 'logs' }))).toBe('logs · b1:9092,b2:9092');
    expect(placeOf(connector('aws_cloudwatch_logs', { region: 'us-east-1', logGroup: '/aws/fn' }))).toBe(
      '/aws/fn · us-east-1',
    );
    expect(placeOf(connector('kafka_source', { brokers: 'b1:9092', topic: 'logs' }))).toBe('logs · b1:9092');
    expect(placeOf(connector('s3_source', { bucket: 'acme', prefix: 'AWSLogs/' }))).toBe('s3://acme/AWSLogs/');
  });

  it('says where an HTTP destination points without its address, which is a secret', () => {
    expect(placeOf(connector('http', { url: '***', urlOrigin: 'https://collector.example.com' }))).toBe(
      'https://collector.example.com',
    );
  });

  it('names the stream a source writes to, as the receiver falls back when none is set', () => {
    expect(targetStreamOf(connector('cloudflare_logpush', { targetStream: ' edge ' }))).toBe('edge');
    expect(targetStreamOf(connector('cloudflare_logpush', {}))).toBe('cloudflare');
    expect(targetStreamOf(connector('aws_kinesis_firehose', {}))).toBe('kinesis');
    expect(targetStreamOf(connector('heroku_drain', {}))).toBe('heroku');
    expect(targetStreamOf(connector('aws_cloudwatch_logs', {}))).toBe('default');
  });
});
