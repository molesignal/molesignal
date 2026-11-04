import type {
  AssertionOperator,
  ClientKey,
  HeaderValue,
  MonitorAssertion,
  MonitorSpec,
  SyntheticSecret,
  ValueSource,
} from '@/api/synthetics';

import { clientId } from '../model';
import type { AssertionDraft, CheckDraft } from './model';

export function buildSpec(draft: CheckDraft, secrets: SyntheticSecret[]): MonitorSpec {
  const assertions = draft.assertions.map(buildAssertion);
  if (draft.kind === 'http') return httpSpec(draft, secrets, assertions);
  if (draft.kind === 'browser') {
    return {
      kind: 'browser',
      configuration: {
        steps: parseBrowserSteps(draft.browserSteps, secrets),
        viewport: { width: 1440, height: 900 },
        captureScreenshotOnFailure: true,
        captureHarOnFailure: true,
        captureTraceOnFailure: true,
      },
    };
  }
  if (draft.kind === 'tcp') {
    return {
      kind: 'tcp',
      configuration: {
        host: valueSource(draft.host, secrets),
        port: Number(draft.port) || 443,
        useTls: draft.useTls,
        assertions,
      },
    };
  }
  if (draft.kind === 'ssh') {
    const authentication = draft.sshAuthMode === 'password'
      ? {
          kind: 'password' as const,
          username: valueSource(draft.sshUsername, secrets),
          password: valueSource(draft.sshPassword, secrets),
        }
      : draft.sshAuthMode === 'public_key'
        ? {
            kind: 'public_key' as const,
            username: valueSource(draft.sshUsername, secrets),
            privateKey: valueSource(draft.sshPrivateKey, secrets),
            ...(draft.sshPassphrase.trim()
              ? { passphrase: valueSource(draft.sshPassphrase, secrets) }
              : {}),
          }
        : undefined;
    return {
      kind: 'ssh',
      configuration: {
        host: valueSource(draft.host, secrets),
        port: Number(draft.port) || 22,
        ...(draft.sshIdentificationRegex.trim()
          ? { expectedIdentificationRegex: draft.sshIdentificationRegex.trim() }
          : {}),
        ...(authentication ? { authentication } : {}),
        ...(authentication && draft.sshHostKeySha256.trim()
          ? { expectedHostKeySha256: draft.sshHostKeySha256.trim() }
          : {}),
        ...(authentication && draft.sshCommand.trim()
          ? {
              command: valueSource(draft.sshCommand, secrets),
              expectedExitStatus: Number(draft.sshExitStatus) || 0,
              ...(draft.sshOutputRegex.trim()
                ? { expectedOutputRegex: draft.sshOutputRegex.trim() }
                : {}),
            }
          : {}),
      },
    };
  }
  if (draft.kind === 'dns') {
    return {
      kind: 'dns',
      configuration: {
        name: draft.host.trim(),
        recordType: draft.recordType,
        ...(draft.resolver.trim() ? { resolver: draft.resolver.trim() } : {}),
        requireDnssec: draft.dnssec,
        expectedValues: splitComma(draft.expectedValues),
        assertions,
      },
    };
  }
  if (draft.kind === 'icmp') {
    return {
      kind: 'icmp',
      configuration: {
        host: draft.host.trim(),
        count: Math.max(1, Math.min(10, Number(draft.pingCount) || 4)),
        intervalMillis: 250,
        maxPacketLossRatio: Math.max(
          0,
          Math.min(1, (Number(draft.packetLoss) || 0) / 100),
        ),
        ...(draft.latencyLimit
          ? { maxMeanRttMillis: Number(draft.latencyLimit) }
          : {}),
      },
    };
  }
  if (draft.kind === 'tls') {
    return {
      kind: 'tls',
      configuration: {
        host: draft.host.trim(),
        port: Number(draft.port) || 443,
        minimumDaysRemaining: Math.max(1, Number(draft.certificateDays) || 30),
        expectedSans: [],
        minimumProtocol: 'TLS1.2',
      },
    };
  }
  if (draft.kind === 'grpc') {
    return {
      kind: 'grpc',
      configuration: {
        endpoint: draft.host.trim(),
        useTls: draft.useTls,
        metadata: [],
        call: { kind: 'health', service: draft.grpcService.trim() },
        assertions,
      },
    };
  }
  return { kind: 'heartbeat' };
}

function httpSpec(
  draft: CheckDraft,
  secrets: SyntheticSecret[],
  assertions: MonitorAssertion[],
): MonitorSpec {
  const headers: HeaderValue[] = draft.headers
    .filter((header) => header.name.trim())
    .map((header) => ({
      name: header.name.trim(),
      value: valueSource(header.value, secrets),
    }));
  return {
    kind: 'http',
    configuration: {
      steps: [
        {
          id: clientId(),
          name: 'Request',
          method: draft.method,
          url: valueSource(draft.url, secrets),
          headers,
          query: [],
          ...(draft.body.trim() ? { body: valueSource(draft.body, secrets) } : {}),
          extractions: [],
          assertions,
        },
      ],
      followRedirects: true,
      maxRedirects: 5,
      verifyTls: draft.useTls,
      httpVersion: draft.httpVersion,
      ...clientCertificate(draft),
      ...(draft.dnsServer.trim() ? { dnsServer: draft.dnsServer.trim() } : {}),
      hosts: draft.hostBindings
        .filter((binding) => binding.name.trim())
        .map((binding) => ({
          name: binding.name.trim(),
          addresses: binding.addresses
            .split(/[\s,]+/)
            .map((address) => address.trim())
            .filter(Boolean),
        })),
    },
  };
}

/**
 * The client certificate as typed or uploaded, with a new private key or the one the check's
 * revision already holds. Without a certificate, or a key for it, the check shows none.
 */
function clientCertificate(draft: CheckDraft) {
  const certificate = draft.clientCertificate.trim();
  const pem = draft.clientKey.trim();

  const privateKey: ClientKey | null = pem
    ? { state: 'new', pem }
    : draft.clientKeyRevision
      ? { state: 'kept', revisionId: draft.clientKeyRevision }
      : null;

  return certificate && privateKey
    ? { clientCertificate: { certificate, privateKey } }
    : {};
}

function buildAssertion(draft: AssertionDraft): MonitorAssertion {
  let operator: AssertionOperator;
  if (draft.operator === 'exists') {
    operator = { operator: 'exists' };
  } else if (draft.operator === 'matches') {
    operator = { operator: 'matches', pattern: draft.expected };
  } else if (draft.operator === 'greater_than' || draft.operator === 'less_than') {
    operator = { operator: draft.operator, expected: Number(draft.expected) || 0 };
  } else if (draft.operator === 'json_schema') {
    let schema: unknown = {};
    try {
      schema = JSON.parse(draft.expected);
    } catch {
      schema = {};
    }
    operator = { operator: 'json_schema', schema };
  } else {
    operator = { operator: draft.operator, expected: draft.expected };
  }
  return {
    id: draft.id || clientId(),
    name: draft.name.trim() || `${draft.source} ${draft.operator}`,
    source: draft.source.trim(),
    severity: draft.severity,
    operator,
  };
}

function parseBrowserSteps(source: string, secrets: SyntheticSecret[]) {
  return source
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line, index) => {
      const [verb = 'wait', first = '', ...rest] = line.split(/\s+/);
      const tail = rest.join(' ');
      let action: Record<string, unknown> & { action: string };
      if (verb === 'open') {
        action = {
          action: 'navigate',
          url: valueSource(first, secrets),
          waitUntil: 'networkidle',
        };
      } else if (verb === 'click') {
        action = { action: 'click', selector: [first, ...rest].join(' ') };
      } else if (verb === 'input') {
        action = {
          action: 'fill',
          selector: first,
          value: valueSource(tail, secrets),
        };
      } else if (verb === 'screenshot') {
        action = {
          action: 'screenshot',
          name: [first, ...rest].join(' ') || `step-${index + 1}`,
          fullPage: true,
        };
      } else if (verb === 'wait' && /^\d+$/.test(first)) {
        action = {
          action: 'wait_duration',
          durationMillis: Math.min(30_000, Number(first)),
        };
      } else {
        action = { action: 'wait_selector', selector: [first, ...rest].join(' ') };
      }
      return { id: clientId(), name: `${index + 1}. ${verb}`, action };
    });
}

function valueSource(value: string, secrets: SyntheticSecret[]): ValueSource {
  const normalized = value.trim();
  const reference = /^\{\{([A-Za-z_][A-Za-z0-9_]*)\}\}$/.exec(normalized)?.[1];
  if (!reference) return { source: 'literal', value: normalized };
  const secret = secrets.find((item) => item.name === reference);
  return secret
    ? { source: 'secret', reference, secretId: secret.id }
    : { source: 'variable', name: reference };
}

function splitComma(value: string): string[] {
  return value
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean);
}
