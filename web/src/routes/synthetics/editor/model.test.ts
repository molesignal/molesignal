import { describe, expect, it } from 'vitest';

import type { SyntheticSecret } from '@/api/synthetics';

import { emptyDraft, inputFromDraft } from './model';

const SECRET: SyntheticSecret = {
  id: 'secret-api-key',
  organizationId: 'org-a',
  name: 'API_KEY',
  description: 'Test credential',
  currentVersion: 2,
  createdBy: 'user-a',
  createdAt: 1,
  updatedAt: 2,
};

describe('synthetics editor model', () => {
  it('keeps runtime variables and encrypted secrets distinct', () => {
    const draft = {
      ...emptyDraft([]),
      name: 'Checkout API',
      url: '{{base_url}}',
      headers: [{ name: 'Authorization', value: '{{API_KEY}}' }],
      locationIds: ['location-a'],
    };

    const input = inputFromDraft(draft, [SECRET]);

    expect(input.spec.kind).toBe('http');
    if (input.spec.kind !== 'http') throw new Error('expected HTTP spec');
    const step = input.spec.configuration.steps[0];
    expect(step?.url).toEqual({ source: 'variable', name: 'base_url' });
    expect(step?.headers[0]?.value).toEqual({
      source: 'secret',
      reference: 'API_KEY',
      secretId: SECRET.id,
    });
  });

  it('enforces the browser cadence and timeout boundary', () => {
    const input = inputFromDraft({
      ...emptyDraft([]),
      name: 'Checkout journey',
      kind: 'browser',
      browserSteps: 'open https://example.com',
      intervalSeconds: '30',
      timeoutMillis: '90000',
      locationIds: ['location-a'],
    });

    expect(input.schedule).toEqual({ kind: 'interval', everySeconds: 60 });
    expect(input.timeoutMillis).toBe(59_999);
  });

  it('creates a location-free heartbeat schedule', () => {
    const input = inputFromDraft({
      ...emptyDraft([]),
      name: 'Batch heartbeat',
      kind: 'heartbeat',
      intervalSeconds: '300',
      locationIds: ['ignored-location'],
    });

    expect(input.spec).toEqual({ kind: 'heartbeat' });
    expect(input.schedule).toEqual({
      kind: 'heartbeat',
      expectedSeconds: 300,
      graceSeconds: 600,
    });
    expect(input.locationIds).toEqual([]);
  });

  it('sends a gRPC endpoint as the host and port typed, with TLS from the switch', () => {
    const input = inputFromDraft({
      ...emptyDraft([]),
      name: 'Orders gRPC health',
      kind: 'grpc',
      host: ' orders.example.com:443 ',
      useTls: true,
      grpcService: 'orders.v1.Orders',
      assertions: [],
      locationIds: ['location-a'],
    });

    expect(input.spec).toEqual({
      kind: 'grpc',
      configuration: {
        endpoint: 'orders.example.com:443',
        useTls: true,
        metadata: [],
        call: { kind: 'health', service: 'orders.v1.Orders' },
        assertions: [],
      },
    });
  });

  it('builds an SSH identification check with the standard port', () => {
    const input = inputFromDraft({
      ...emptyDraft([]),
      name: 'Production bastion',
      kind: 'ssh',
      host: '{{bastion_host}}',
      port: '22',
      sshIdentificationRegex: '^SSH-2\\.0-OpenSSH_',
      locationIds: ['private-vpc'],
    });

    expect(input.spec).toEqual({
      kind: 'ssh',
      configuration: {
        host: { source: 'variable', name: 'bastion_host' },
        port: 22,
        expectedIdentificationRegex: '^SSH-2\\.0-OpenSSH_',
      },
    });
  });

  it('builds an SSH authentication and command check with Secret values', () => {
    const input = inputFromDraft({
      ...emptyDraft([]),
      name: 'Production bastion command',
      kind: 'ssh',
      host: 'bastion.internal',
      port: '22',
      sshAuthMode: 'password',
      sshUsername: 'deploy',
      sshPassword: '{{API_KEY}}',
      sshHostKeySha256: `SHA256:${'A'.repeat(43)}`,
      sshCommand: 'systemctl is-active app',
      sshOutputRegex: '^active$',
      sshExitStatus: '0',
      alertOnFlaky: true,
      locationIds: ['private-vpc'],
    }, [SECRET]);

    expect(input.alertOnFlaky).toBe(true);
    // A check alerts when it stops getting results unless told not to.
    expect(input.alertOnNoData).toBe(true);
    expect(input.spec).toEqual({
      kind: 'ssh',
      configuration: {
        host: { source: 'literal', value: 'bastion.internal' },
        port: 22,
        authentication: {
          kind: 'password',
          username: { source: 'literal', value: 'deploy' },
          password: {
            source: 'secret',
            reference: 'API_KEY',
            secretId: SECRET.id,
          },
        },
        expectedHostKeySha256: `SHA256:${'A'.repeat(43)}`,
        command: { source: 'literal', value: 'systemctl is-active app' },
        expectedOutputRegex: '^active$',
        expectedExitStatus: 0,
      },
    });
  });

  it('sends the HTTP connection settings, the client certificate as written', () => {
    const input = inputFromDraft({
      ...emptyDraft([]),
      name: 'Partner API',
      url: 'https://api.example.com/health',
      httpVersion: 'http2',
      clientCertificate: ' -----BEGIN CERTIFICATE-----\n ',
      clientKey: '-----BEGIN PRIVATE KEY-----\n',
      dnsServer: ' 1.1.1.1:53 ',
      hostBindings: [
        { name: ' api.example.com ', addresses: '192.0.2.10, 2001:db8::1' },
        { name: '', addresses: '192.0.2.11' },
      ],
      locationIds: ['location-a'],
    });

    if (input.spec.kind !== 'http') throw new Error('expected HTTP spec');
    expect(input.spec.configuration).toMatchObject({
      httpVersion: 'http2',
      clientCertificate: {
        certificate: '-----BEGIN CERTIFICATE-----',
        privateKey: { state: 'new', pem: '-----BEGIN PRIVATE KEY-----' },
      },
      dnsServer: '1.1.1.1:53',
      hosts: [{ name: 'api.example.com', addresses: ['192.0.2.10', '2001:db8::1'] }],
    });
  });

  it('keeps the saved private key unless a new one is given', () => {
    const draft = {
      ...emptyDraft([]),
      name: 'Partner API',
      url: 'https://api.example.com/health',
      clientCertificate: '-----BEGIN CERTIFICATE-----',
      clientKeyRevision: 'revision-3',
      locationIds: ['location-a'],
    };

    const kept = inputFromDraft(draft).spec;
    const replaced = inputFromDraft({ ...draft, clientKey: '-----BEGIN PRIVATE KEY-----' }).spec;
    const dropped = inputFromDraft({ ...draft, clientCertificate: '' }).spec;

    if (kept.kind !== 'http' || replaced.kind !== 'http' || dropped.kind !== 'http') {
      throw new Error('expected HTTP specs');
    }
    expect(kept.configuration.clientCertificate?.privateKey).toEqual({
      state: 'kept',
      revisionId: 'revision-3',
    });
    expect(replaced.configuration.clientCertificate?.privateKey.state).toBe('new');
    expect(dropped.configuration.clientCertificate).toBeUndefined();
  });
});
