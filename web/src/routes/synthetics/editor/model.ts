import type {
  AssertionOperator,
  AssertionSeverity,
  CreateMonitorInput,
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

export interface CheckDraft {
  name: string;
  description: string;
  kind: MonitorKind;
  method: string;
  url: string;
  body: string;
  headers: HeaderDraft[];
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
    timeoutMillis: String(input.timeout_millis),
    retries: String(input.max_retries),
    failureThreshold: String(input.consecutive_failures),
    recoveryThreshold: String(input.consecutive_recoveries),
    locationIds: input.location_ids,
    tags: input.tags.join(', '),
    alertOnDegraded: input.alert_on_degraded,
    alertOnFlaky: input.alert_on_flaky,
    escalationPolicyId: input.escalation_policy_id ?? '',
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
              ? revision.schedule.expected_seconds
              : revision.schedule.every_seconds,
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
  } else if (spec.kind === 'browser') {
    draft.browserSteps = spec.configuration.steps.map(browserStepText).join('\n');
  } else if (spec.kind === 'tcp') {
    draft.host = valueSourceText(spec.configuration.host);
    draft.port = String(spec.configuration.port);
    draft.useTls = spec.configuration.use_tls;
    draft.assertions = spec.configuration.assertions.map(assertionToDraft);
  } else if (spec.kind === 'ssh') {
    draft.host = valueSourceText(spec.configuration.host);
    draft.port = String(spec.configuration.port);
    draft.sshIdentificationRegex = spec.configuration.expected_identification_regex ?? '';
    draft.sshHostKeySha256 = spec.configuration.expected_host_key_sha256 ?? '';
    draft.sshCommand = spec.configuration.command
      ? valueSourceText(spec.configuration.command)
      : '';
    draft.sshOutputRegex = spec.configuration.expected_output_regex ?? '';
    draft.sshExitStatus = String(spec.configuration.expected_exit_status ?? 0);
    const authentication = spec.configuration.authentication;
    if (authentication?.kind === 'password') {
      draft.sshAuthMode = 'password';
      draft.sshUsername = valueSourceText(authentication.username);
      draft.sshPassword = valueSourceText(authentication.password);
    } else if (authentication?.kind === 'public_key') {
      draft.sshAuthMode = 'public_key';
      draft.sshUsername = valueSourceText(authentication.username);
      draft.sshPrivateKey = valueSourceText(authentication.private_key);
      draft.sshPassphrase = authentication.passphrase
        ? valueSourceText(authentication.passphrase)
        : '';
    }
  } else if (spec.kind === 'dns') {
    draft.host = spec.configuration.name;
    draft.recordType = spec.configuration.record_type;
    draft.resolver = spec.configuration.resolver ?? '';
    draft.expectedValues = spec.configuration.expected_values.join(', ');
    draft.dnssec = spec.configuration.require_dnssec;
    draft.assertions = spec.configuration.assertions.map(assertionToDraft);
  } else if (spec.kind === 'icmp') {
    draft.host = spec.configuration.host;
    draft.pingCount = String(spec.configuration.count);
    draft.packetLoss = String(spec.configuration.max_packet_loss_ratio * 100);
    draft.latencyLimit = String(spec.configuration.max_mean_rtt_millis ?? '');
  } else if (spec.kind === 'tls') {
    draft.host = spec.configuration.host;
    draft.port = String(spec.configuration.port);
    draft.certificateDays = String(spec.configuration.minimum_days_remaining);
  } else if (spec.kind === 'grpc') {
    draft.host = spec.configuration.endpoint;
    draft.useTls = spec.configuration.use_tls;
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
  if (action.action === 'wait_duration') return `wait ${String(action.duration_millis ?? 1000)}`;
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
        ? { kind: 'heartbeat', expected_seconds: interval, grace_seconds: interval * 2 }
        : draft.scheduleKind === 'cron'
          ? { kind: 'cron', expression: draft.cron.trim(), timezone: draft.timezone }
          : { kind: 'interval', every_seconds: interval },
    timeout_millis: timeout,
    max_retries: Math.max(0, Math.min(2, Number(draft.retries) || 0)),
    consecutive_failures: Math.max(1, Number(draft.failureThreshold) || 1),
    consecutive_recoveries: Math.max(1, Number(draft.recoveryThreshold) || 1),
    freshness_seconds: Math.max(interval * 3, 60),
    location_policy: { kind: 'majority' },
    location_ids: draft.kind === 'heartbeat' ? [] : draft.locationIds,
    tags: draft.tags.split(',').map((tag) => tag.trim()).filter(Boolean),
    ...(draft.escalationPolicyId
      ? { escalation_policy_id: draft.escalationPolicyId }
      : {}),
    alert_on_degraded: draft.alertOnDegraded,
    alert_on_flaky: draft.alertOnFlaky,
  };
}
