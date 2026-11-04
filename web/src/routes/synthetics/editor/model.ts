import type {
  AssertionOperator,
  AssertionSeverity,
  CreateMonitorInput,
  HttpVersion,
  MonitorAssertion,
  MonitorDetail,
  MonitorKind,
  MonitorSpec,
  ProbeLocation,
  SyntheticSecret,
  ValueSource,
} from '@/api/synthetics';

import { activeRevision, clientId, detailToInput, valueSourceText } from '../model';
import { buildSpec } from './spec';

export interface AssertionDraft {
  id: string;
  name: string;
  source: string;
  operator: AssertionOperator['operator'];
  expected: string;
  severity: AssertionSeverity;
}

export interface HeaderDraft {
  name: string;
  value: string;
}

/** A host name and the addresses it is reached at, comma separated as typed. */
export interface HostBindingDraft {
  name: string;
  addresses: string;
}

export interface CheckDraft {
  name: string;
  description: string;
  kind: MonitorKind;
  method: string;
  url: string;
  body: string;
  headers: HeaderDraft[];
  httpVersion: HttpVersion;
  /** The client certificate chain, in PEM. */
  clientCertificate: string;
  /** A new private key in PEM; empty keeps the key the revision below holds, if any. */
  clientKey: string;
  /** The revision holding the saved private key, which is never sent back. */
  clientKeyRevision: string | null;
  dnsServer: string;
  hostBindings: HostBindingDraft[];
  browserSteps: string;
  host: string;
  port: string;
  sshIdentificationRegex: string;
  sshAuthMode: 'none' | 'password' | 'public_key';
  sshUsername: string;
  sshPassword: string;
  sshPrivateKey: string;
  sshPassphrase: string;
  sshHostKeySha256: string;
  sshCommand: string;
  sshOutputRegex: string;
  sshExitStatus: string;
  recordType: string;
  resolver: string;
  expectedValues: string;
  dnssec: boolean;
  useTls: boolean;
  pingCount: string;
  packetLoss: string;
  latencyLimit: string;
  certificateDays: string;
  grpcService: string;
  assertions: AssertionDraft[];
  scheduleKind: 'interval' | 'cron';
  intervalSeconds: string;
  cron: string;
  timezone: string;
  timeoutMillis: string;
  retries: string;
  failureThreshold: string;
  recoveryThreshold: string;
  locationIds: string[];
  tags: string;
  alertOnDegraded: boolean;
  alertOnFlaky: boolean;
  alertOnNoData: boolean;
  injectTraceContext: boolean;
  tracerouteOnFailure: boolean;
  escalationPolicyId: string;
}

const DEFAULT_ASSERTION: AssertionDraft = {
  id: '',
  name: 'HTTP 200',
  source: 'status',
  operator: 'equals',
  expected: '200',
  severity: 'critical',
};

export function emptyDraft(locations: ProbeLocation[]): CheckDraft {
  return {
    name: '',
    description: '',
    kind: 'http',
    method: 'GET',
    url: 'https://',
    body: '',
    headers: [],
    httpVersion: 'auto',
    clientCertificate: '',
    clientKey: '',
    clientKeyRevision: null,
    dnsServer: '',
    hostBindings: [],
    browserSteps: 'open https://',
    host: '',
    port: '443',
    sshIdentificationRegex: '',
    sshAuthMode: 'none',
    sshUsername: '',
    sshPassword: '',
    sshPrivateKey: '',
    sshPassphrase: '',
    sshHostKeySha256: '',
    sshCommand: '',
    sshOutputRegex: '',
    sshExitStatus: '0',
    recordType: 'A',
    resolver: '',
    expectedValues: '',
    dnssec: false,
    useTls: true,
    pingCount: '4',
    packetLoss: '0',
    latencyLimit: '500',
    certificateDays: '30',
    grpcService: '',
    assertions: [{ ...DEFAULT_ASSERTION, id: clientId() }],
    scheduleKind: 'interval',
    intervalSeconds: '60',
    cron: '*/5 * * * *',
    timezone: 'UTC',
    timeoutMillis: '10000',
    retries: '0',
    failureThreshold: '3',
    recoveryThreshold: '2',
    locationIds: locations.filter((location) => location.lifecycle === 'active').slice(0, 1).map((location) => location.id),
    tags: '',
    alertOnDegraded: false,
    alertOnFlaky: false,
    alertOnNoData: true,
    injectTraceContext: true,
    tracerouteOnFailure: true,
    escalationPolicyId: '',
  };
}

export function applyPreset(draft: CheckDraft, preset: string | null): CheckDraft {
  if (!preset || preset === 'status') return draft;
  const next = { ...draft, assertions: draft.assertions.slice() };
  if (preset === 'latency') {
    next.assertions = [{
      id: clientId(), name: 'duration_ms < 500', source: 'duration_ms',
      operator: 'less_than', expected: '500', severity: 'critical',
    }];
  } else if (preset === 'regex') {
    next.assertions = [{
      id: clientId(), name: 'body regex', source: 'body',
      operator: 'matches', expected: 'success', severity: 'critical',
    }];
  } else if (preset === 'header' || preset === 'cookie') {
    next.assertions = [{
      id: clientId(), name: preset === 'cookie' ? 'header:set-cookie exists' : 'header:content-type exists',
      source: preset === 'cookie' ? 'header:set-cookie' : 'header:content-type',
      operator: 'exists', expected: '', severity: 'critical',
    }];
  } else if (preset === 'tls') {
    next.kind = 'tls';
    next.assertions = [];
  } else if (preset === 'dns') {
    next.kind = 'dns';
    next.assertions = [];
  } else if (preset === 'json') {
    next.assertions = [{
      id: clientId(), name: 'JSON schema', source: 'body',
      operator: 'json_schema', expected: '{}', severity: 'critical',
    }];
  }
  return next;
}

export function draftFromDetail(
  detail: MonitorDetail,
  locations: ProbeLocation[],
  clone: boolean,
): CheckDraft {
  const base = emptyDraft(locations);
  const input = detailToInput(detail, clone);
  const revision = activeRevision(detail);
  if (!input || !revision) return base;
  const draft: CheckDraft = {
    ...base,
    name: input.name,
    description: input.description,
    kind: input.spec.kind,
    timeoutMillis: String(input.timeoutMillis),
    retries: String(input.maxRetries),
    failureThreshold: String(input.consecutiveFailures),
    recoveryThreshold: String(input.consecutiveRecoveries),
    locationIds: input.locationIds,
    tags: input.tags.join(', '),
    alertOnDegraded: input.alertOnDegraded,
    alertOnFlaky: input.alertOnFlaky,
    alertOnNoData: input.alertOnNoData,
    injectTraceContext: input.injectTraceContext,
    tracerouteOnFailure: input.tracerouteOnFailure,
    escalationPolicyId: input.escalationPolicyId ?? '',
    ...(revision.schedule.kind === 'cron'
      ? {
          scheduleKind: 'cron' as const,
          cron: revision.schedule.expression,
          timezone: revision.schedule.timezone,
        }
      : {
          scheduleKind: 'interval' as const,
          intervalSeconds: String(
            revision.schedule.kind === 'heartbeat'
              ? revision.schedule.expectedSeconds
              : revision.schedule.everySeconds,
          ),
        }),
  };
  hydrateSpec(draft, input.spec);
  return draft;
}

function hydrateSpec(draft: CheckDraft, spec: MonitorSpec) {
  if (spec.kind === 'http') {
    const step = spec.configuration.steps[0];
    draft.method = step?.method ?? 'GET';
    draft.url = valueSourceText(step?.url);
    draft.body = step?.body ? valueSourceText(step.body) : '';
    draft.headers = (step?.headers ?? []).map((header) => ({
      name: header.name,
      value: valueSourceText(header.value),
    }));
    draft.assertions = (step?.assertions ?? []).map(assertionToDraft);
    draft.httpVersion = spec.configuration.httpVersion ?? 'auto';
    const clientCertificate = spec.configuration.clientCertificate;
    draft.clientCertificate = clientCertificate?.certificate ?? '';
    draft.clientKey = '';
    draft.clientKeyRevision =
      clientCertificate?.privateKey.state === 'kept' ? clientCertificate.privateKey.revisionId : null;
    draft.dnsServer = spec.configuration.dnsServer ?? '';
    draft.hostBindings = (spec.configuration.hosts ?? []).map((binding) => ({
      name: binding.name,
      addresses: binding.addresses.join(', '),
    }));
  } else if (spec.kind === 'browser') {
    draft.browserSteps = spec.configuration.steps.map(browserStepText).join('\n');
  } else if (spec.kind === 'tcp') {
    draft.host = valueSourceText(spec.configuration.host);
    draft.port = String(spec.configuration.port);
    draft.useTls = spec.configuration.useTls;
    draft.assertions = spec.configuration.assertions.map(assertionToDraft);
  } else if (spec.kind === 'ssh') {
    draft.host = valueSourceText(spec.configuration.host);
    draft.port = String(spec.configuration.port);
    draft.sshIdentificationRegex = spec.configuration.expectedIdentificationRegex ?? '';
    draft.sshHostKeySha256 = spec.configuration.expectedHostKeySha256 ?? '';
    draft.sshCommand = spec.configuration.command
      ? valueSourceText(spec.configuration.command)
      : '';
    draft.sshOutputRegex = spec.configuration.expectedOutputRegex ?? '';
    draft.sshExitStatus = String(spec.configuration.expectedExitStatus ?? 0);
    const authentication = spec.configuration.authentication;
    if (authentication?.kind === 'password') {
      draft.sshAuthMode = 'password';
      draft.sshUsername = valueSourceText(authentication.username);
      draft.sshPassword = valueSourceText(authentication.password);
    } else if (authentication?.kind === 'public_key') {
      draft.sshAuthMode = 'public_key';
      draft.sshUsername = valueSourceText(authentication.username);
      draft.sshPrivateKey = valueSourceText(authentication.privateKey);
      draft.sshPassphrase = authentication.passphrase
        ? valueSourceText(authentication.passphrase)
        : '';
    }
  } else if (spec.kind === 'dns') {
    draft.host = spec.configuration.name;
    draft.recordType = spec.configuration.recordType;
    draft.resolver = spec.configuration.resolver ?? '';
    draft.expectedValues = spec.configuration.expectedValues.join(', ');
    draft.dnssec = spec.configuration.requireDnssec;
    draft.assertions = spec.configuration.assertions.map(assertionToDraft);
  } else if (spec.kind === 'icmp') {
    draft.host = spec.configuration.host;
    draft.pingCount = String(spec.configuration.count);
    draft.packetLoss = String(spec.configuration.maxPacketLossRatio * 100);
    draft.latencyLimit = String(spec.configuration.maxMeanRttMillis ?? '');
  } else if (spec.kind === 'tls') {
    draft.host = spec.configuration.host;
    draft.port = String(spec.configuration.port);
    draft.certificateDays = String(spec.configuration.minimumDaysRemaining);
  } else if (spec.kind === 'grpc') {
    draft.host = spec.configuration.endpoint;
    draft.useTls = spec.configuration.useTls;
    draft.grpcService = spec.configuration.call.service;
    draft.assertions = spec.configuration.assertions.map(assertionToDraft);
  }
}

function assertionToDraft(assertion: MonitorAssertion): AssertionDraft {
  const operator = assertion.operator;
  const expected =
    'expected' in operator
      ? String(operator.expected)
      : 'pattern' in operator
        ? operator.pattern
        : 'schema' in operator
          ? JSON.stringify(operator.schema)
          : '';
  return {
    id: assertion.id,
    name: assertion.name,
    source: assertion.source,
    operator: operator.operator,
    expected,
    severity: assertion.severity,
  };
}

function browserStepText(step: { action: Record<string, unknown> & { action: string } }): string {
  const action = step.action;
  if (action.action === 'navigate') return `open ${valueSourceText(action.url as ValueSource)}`;
  if (action.action === 'click') return `click ${String(action.selector ?? '')}`;
  if (action.action === 'fill') {
    return `input ${String(action.selector ?? '')} ${valueSourceText(action.value as ValueSource)}`;
  }
  if (action.action === 'wait_duration') return `wait ${String(action.durationMillis ?? 1000)}`;
  if (action.action === 'wait_selector') return `wait ${String(action.selector ?? '')}`;
  if (action.action === 'screenshot') return `screenshot ${String(action.name ?? 'capture')}`;
  return `${action.action} ${String(action.selector ?? action.expression ?? '')}`.trim();
}

export function inputFromDraft(
  draft: CheckDraft,
  secrets: SyntheticSecret[] = [],
): CreateMonitorInput {
  const interval = Math.max(draft.kind === 'browser' ? 60 : 10, Number(draft.intervalSeconds) || 60);
  const requestedTimeout = Math.max(100, Math.min(300_000, Number(draft.timeoutMillis) || 10_000));
  const timeout = draft.scheduleKind === 'interval'
    ? Math.min(requestedTimeout, interval * 1000 - 1)
    : requestedTimeout;
  return {
    name: draft.name.trim(),
    description: draft.description.trim(),
    spec: buildSpec(draft, secrets),
    schedule:
      draft.kind === 'heartbeat'
        ? { kind: 'heartbeat', expectedSeconds: interval, graceSeconds: interval * 2 }
        : draft.scheduleKind === 'cron'
          ? { kind: 'cron', expression: draft.cron.trim(), timezone: draft.timezone }
          : { kind: 'interval', everySeconds: interval },
    timeoutMillis: timeout,
    maxRetries: Math.max(0, Math.min(2, Number(draft.retries) || 0)),
    consecutiveFailures: Math.max(1, Number(draft.failureThreshold) || 1),
    consecutiveRecoveries: Math.max(1, Number(draft.recoveryThreshold) || 1),
    freshnessSeconds: Math.max(interval * 3, 60),
    locationPolicy: { kind: 'majority' },
    locationIds: draft.kind === 'heartbeat' ? [] : draft.locationIds,
    tags: draft.tags.split(',').map((tag) => tag.trim()).filter(Boolean),
    ...(draft.escalationPolicyId
      ? { escalationPolicyId: draft.escalationPolicyId }
      : {}),
    alertOnDegraded: draft.alertOnDegraded,
    alertOnFlaky: draft.alertOnFlaky,
    alertOnNoData: draft.alertOnNoData,
    injectTraceContext: draft.injectTraceContext,
    tracerouteOnFailure: draft.tracerouteOnFailure,
  };
}
