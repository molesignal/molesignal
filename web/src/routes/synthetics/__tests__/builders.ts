import type {
  MonitorRevision,
  ProbeAgent,
  ProbeLocation,
  SyntheticMonitor,
  SyntheticResult,
} from '@/api/synthetics';

import type { CheckRow } from '../data';

/** One minute in microseconds: the step the builders space runs by. */
export const MINUTE = 60_000_000;

/** A moment the builders count back from, so relative times in tests are stable. */
export const NOW = Date.UTC(2026, 9, 11, 12) * 1_000;

export function monitor(overrides: Partial<SyntheticMonitor> = {}): SyntheticMonitor {
  return {
    id: 'check-1',
    organizationId: 'org-1',
    name: 'Checkout API',
    description: '',
    kind: 'http',
    lifecycle: 'active',
    state: 'healthy',
    tags: [],
    activeRevisionId: 'revision-1',
    createdBy: 'user-1',
    createdAt: NOW - 30 * 24 * 60 * MINUTE,
    updatedAt: NOW - 60 * MINUTE,
    ...overrides,
  };
}

export function revision(overrides: Partial<MonitorRevision> = {}): MonitorRevision {
  return {
    id: 'revision-1',
    organizationId: 'org-1',
    monitorId: 'check-1',
    number: 1,
    spec: {
      kind: 'tcp',
      configuration: { host: { source: 'literal', value: 'db.acme.corp' }, port: 5432 },
    } as MonitorRevision['spec'],
    schedule: { kind: 'interval', everySeconds: 60 },
    timeoutMillis: 10_000,
    maxRetries: 1,
    consecutiveFailures: 2,
    consecutiveRecoveries: 2,
    freshnessSeconds: 300,
    locationPolicy: { kind: 'majority' },
    locationIds: ['loc-us', 'loc-eu'],
    alertOnDegraded: true,
    alertOnFlaky: false,
    alertOnNoData: true,
    injectTraceContext: true,
    tracerouteOnFailure: true,
    createdBy: 'user-1',
    createdAt: NOW - 30 * 24 * 60 * MINUTE,
    contentHash: 'abcdef0123456789abcdef',
    ...overrides,
  };
}

export function result(overrides: Partial<SyntheticResult> = {}): SyntheticResult {
  const startedAt = overrides.startedAt ?? NOW - MINUTE;

  return {
    id: `result-${startedAt}-${overrides.locationId ?? 'loc-us'}`,
    organizationId: 'org-1',
    monitorId: 'check-1',
    monitorRevisionId: 'revision-1',
    locationId: 'loc-us',
    taskId: 'task-1',
    isTest: false,
    scheduledAt: startedAt,
    startedAt: startedAt,
    finishedAt: startedAt + 200_000,
    receivedAt: startedAt + 250_000,
    outcome: 'healthy',
    attempts: [],
    assertions: [],
    secretVersions: {},
    protocolVersion: 1,
    metadata: {},
    ...overrides,
  };
}

/**
 * Runs a minute apart, the latest first, from the outcomes given latest first:
 * `runs(['failing', 'failing', 'healthy'])` failed twice after a pass.
 */
export function runs(
  outcomes: ReadonlyArray<SyntheticResult['outcome']>,
  overrides: Partial<SyntheticResult> = {},
): SyntheticResult[] {
  return outcomes.map((outcome, index) =>
    result({ ...overrides, outcome, startedAt: NOW - (index + 1) * MINUTE }),
  );
}

export function checkRow({
  monitor: monitorOverrides = {},
  revision: revisionOverrides = {},
  results = [],
}: {
  monitor?: Partial<SyntheticMonitor>;
  revision?: Partial<MonitorRevision>;
  results?: SyntheticResult[];
} = {}): CheckRow {
  const check = monitor(monitorOverrides);
  const checkRevision = revision({ monitorId: check.id, ...revisionOverrides });
  const scheduled = results
    .filter((item) => !item.isTest)
    .sort((left, right) => right.startedAt - left.startedAt);
  const latest = scheduled[0];
  const decided = scheduled.filter(
    (item) => item.outcome !== 'unknown' && item.outcome !== 'skipped',
  );

  return {
    monitor: check,
    detail: { monitor: check, revisions: [checkRevision] },
    revision: checkRevision,
    results,
    latestResult: latest,
    latestLatencyMicros: latest ? latest.finishedAt - latest.startedAt : undefined,
    successRate: decided.length
      ? decided.filter((item) => item.outcome === 'healthy').length / decided.length
      : undefined,
  };
}

export function location(overrides: Partial<ProbeLocation> = {}): ProbeLocation {
  return {
    id: 'loc-us',
    name: 'N. Virginia',
    code: 'us-east-1',
    description: 'Public probes on the US east coast.',
    scope: 'platform',
    execution: 'agent_pool',
    lifecycle: 'active',
    health: 'online',
    systemManaged: true,
    egressPolicy: {
      allowedCidrs: [],
      deniedCidrs: [],
      allowedDomains: [],
      deniedDomains: [],
      allowedPorts: [],
      allowPrivateNetworks: false,
      allowLoopback: false,
    },
    createdAt: NOW - 90 * 24 * 60 * MINUTE,
    updatedAt: NOW - 120 * MINUTE,
    ...overrides,
  };
}

export function agent(overrides: Partial<ProbeAgent> = {}): ProbeAgent {
  return {
    id: 'agent-1',
    locationId: 'loc-us',
    name: 'probe-a',
    hostname: 'probe-a.acme.internal',
    status: 'online',
    agentVersion: '0.9.2',
    protocolVersion: 3,
    capabilities: ['http', 'tcp'],
    capacity: { maxConcurrent: 16, maxBrowserConcurrent: 2, available: 12, availableBrowser: 2 },
    labels: {},
    lastHeartbeatAt: NOW - 9_000_000,
    lastResultSequence: 1,
    createdAt: NOW - 90 * 24 * 60 * MINUTE,
    updatedAt: NOW - 9_000_000,
    ...overrides,
  };
}
