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
    organization_id: 'org-1',
    name: 'Checkout API',
    description: '',
    kind: 'http',
    lifecycle: 'active',
    state: 'healthy',
    tags: [],
    active_revision_id: 'revision-1',
    created_by: 'user-1',
    created_at: NOW - 30 * 24 * 60 * MINUTE,
    updated_at: NOW - 60 * MINUTE,
    ...overrides,
  };
}

export function revision(overrides: Partial<MonitorRevision> = {}): MonitorRevision {
  return {
    id: 'revision-1',
    organization_id: 'org-1',
    monitor_id: 'check-1',
    number: 1,
    spec: {
      kind: 'tcp',
      configuration: { host: { source: 'literal', value: 'db.acme.corp' }, port: 5432 },
    } as MonitorRevision['spec'],
    schedule: { kind: 'interval', every_seconds: 60 },
    timeout_millis: 10_000,
    max_retries: 1,
    consecutive_failures: 2,
    consecutive_recoveries: 2,
    freshness_seconds: 300,
    location_policy: { kind: 'majority' },
    location_ids: ['loc-us', 'loc-eu'],
    alert_on_degraded: true,
    alert_on_flaky: false,
    created_by: 'user-1',
    created_at: NOW - 30 * 24 * 60 * MINUTE,
    content_hash: 'abcdef0123456789abcdef',
    ...overrides,
  };
}

export function result(overrides: Partial<SyntheticResult> = {}): SyntheticResult {
  const startedAt = overrides.started_at ?? NOW - MINUTE;

  return {
    id: `result-${startedAt}-${overrides.location_id ?? 'loc-us'}`,
    organization_id: 'org-1',
    monitor_id: 'check-1',
    monitor_revision_id: 'revision-1',
    location_id: 'loc-us',
    task_id: 'task-1',
    is_test: false,
    scheduled_at: startedAt,
    started_at: startedAt,
    finished_at: startedAt + 200_000,
    received_at: startedAt + 250_000,
    outcome: 'healthy',
    attempts: [],
    assertions: [],
    secret_versions: {},
    protocol_version: 1,
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
    result({ ...overrides, outcome, started_at: NOW - (index + 1) * MINUTE }),
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
  const checkRevision = revision({ monitor_id: check.id, ...revisionOverrides });
  const scheduled = results
    .filter((item) => !item.is_test)
    .sort((left, right) => right.started_at - left.started_at);
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
    latestLatencyMicros: latest ? latest.finished_at - latest.started_at : undefined,
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
    system_managed: true,
    egress_policy: {
      allowed_cidrs: [],
      denied_cidrs: [],
      allowed_domains: [],
      denied_domains: [],
      allowed_ports: [],
      allow_private_networks: false,
      allow_loopback: false,
    },
    created_at: NOW - 90 * 24 * 60 * MINUTE,
    updated_at: NOW - 120 * MINUTE,
    ...overrides,
  };
}

export function agent(overrides: Partial<ProbeAgent> = {}): ProbeAgent {
  return {
    id: 'agent-1',
    location_id: 'loc-us',
    name: 'probe-a',
    hostname: 'probe-a.acme.internal',
    status: 'online',
    agent_version: '0.9.2',
    protocol_version: 3,
    capabilities: ['http', 'tcp'],
    capacity: { max_concurrent: 16, max_browser_concurrent: 2, available: 12, available_browser: 2 },
    labels: {},
    last_heartbeat_at: NOW - 9_000_000,
    last_result_sequence: 1,
    created_at: NOW - 90 * 24 * 60 * MINUTE,
    updated_at: NOW - 9_000_000,
    ...overrides,
  };
}
