/**
 * Mock data for the two modules that tell the outside world how a system is
 * doing: Synthetics (checks run from probe locations) and Status pages (what
 * customers are told). One made-up company, "Acme Cloud", is described
 * consistently across both, so a screenshot of either reads as a real system
 * and not as rows of lorem ipsum.
 *
 * Every time is relative to the mock session's frozen clock.
 */
import type { Express, Request } from 'express';

import type {
  ActiveAutomationRule,
  AutomationCandidate,
  ComponentStatus,
  PublicStatusPageSnapshot,
  StatusPage,
  StatusPageComponent,
  StatusPageComponentStatusEvent,
  StatusPageDelivery,
  StatusPageIncident,
  StatusPageSnapshot,
  StatusPageSubscriber,
} from '../../src/api/statusPages/types';
import type {
  MonitorKind,
  MonitorRevision,
  MonitorSpec,
  MonitorState,
  ProbeAgent,
  ProbeAgentToken,
  ProbeAttempt,
  ProbeLocation,
  ProbeOutcome,
  SyntheticMonitor,
  SyntheticResult,
  SyntheticSecret,
  TimingBreakdown,
} from '../../src/api/synthetics/types';

const ORG = 'acme-prod';
const MINUTE = 60_000_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** A small deterministic generator: the same "random" data on every run. */
function sequence(seed: number): () => number {
  let state = seed;

  return () => {
    state = (state * 1_664_525 + 1_013_904_223) % 4_294_967_296;
    return state / 4_294_967_296;
  };
}

// ───────────────────────────── Synthetics ─────────────────────────────

const NO_EGRESS_LIMITS = {
  allowedCidrs: [],
  deniedCidrs: [],
  allowedDomains: [],
  deniedDomains: [],
  allowedPorts: [],
  allowPrivateNetworks: false,
  allowLoopback: false,
};

function locations(now: number): ProbeLocation[] {
  const location = (
    id: string,
    name: string,
    code: string,
    description: string,
    overrides: Partial<ProbeLocation> = {},
  ): ProbeLocation => ({
    id,
    name,
    code,
    description,
    scope: 'platform',
    execution: 'agent_pool',
    lifecycle: 'active',
    health: 'online',
    systemManaged: true,
    egressPolicy: NO_EGRESS_LIMITS,
    createdAt: now - 120 * DAY,
    updatedAt: now - 2 * HOUR,
    ...overrides,
  });

  return [
    location('loc-us-east', 'N. Virginia', 'us-east-1', 'Public probes on the US east coast.'),
    location('loc-eu-central', 'Frankfurt', 'eu-central-1', 'Public probes in central Europe.'),
    location('loc-ap-southeast', 'Singapore', 'ap-southeast-1', 'Public probes in south-east Asia.', {
      health: 'degraded',
      updatedAt: now - 12 * MINUTE,
    }),
    location('loc-shanghai', 'Shanghai office', 'cn-sha-office', 'Private probes behind the office firewall.', {
      organizationId: ORG,
      scope: 'organization',
      systemManaged: false,
      egressPolicy: { ...NO_EGRESS_LIMITS, allowPrivateNetworks: true, allowedPorts: [22, 443, 5432] },
      createdAt: now - 40 * DAY,
    }),
  ];
}

function agents(now: number): ProbeAgent[] {
  const agent = (
    id: string,
    location_id: string,
    name: string,
    hostname: string,
    overrides: Partial<ProbeAgent> = {},
  ): ProbeAgent => ({
    id,
    organizationId: location_id === 'loc-shanghai' ? ORG : undefined,
    locationId: location_id,
    name,
    hostname,
    status: 'online',
    agentVersion: '0.9.2',
    protocolVersion: 3,
    featureLevel: 1,
    capabilities: ['http', 'tcp', 'ssh', 'dns', 'icmp', 'tls', 'grpc', 'browser'],
    capacity: { maxConcurrent: 16, maxBrowserConcurrent: 2, available: 13, availableBrowser: 2 },
    labels: { tier: 'production' },
    certificateSerial: `4a:1f:${id.slice(-2)}`,
    certificateExpiresAt: now + 61 * DAY,
    lastHeartbeatAt: now - 9_000_000,
    lastResultSequence: 48_210,
    createdAt: now - 40 * DAY,
    updatedAt: now - 9_000_000,
    ...overrides,
  }) as ProbeAgent;

  return [
    agent('agent-use-01', 'loc-us-east', 'use1-probe-a', 'probe-a.use1.acme.internal'),
    agent('agent-use-02', 'loc-us-east', 'use1-probe-b', 'probe-b.use1.acme.internal', {
      capacity: { maxConcurrent: 16, maxBrowserConcurrent: 2, available: 4, availableBrowser: 0 },
    }),
    agent('agent-euc-01', 'loc-eu-central', 'euc1-probe-a', 'probe-a.euc1.acme.internal'),
    agent('agent-aps-01', 'loc-ap-southeast', 'apse1-probe-a', 'probe-a.apse1.acme.internal', {
      status: 'degraded',
      agentVersion: '0.9.0',
      lastHeartbeatAt: now - 3 * MINUTE,
      certificateExpiresAt: now + 6 * DAY,
    }),
    agent('agent-sha-01', 'loc-shanghai', 'sha-office-1', 'probe-1.sha.acme.corp', {
      capabilities: ['http', 'tcp', 'ssh', 'dns', 'icmp', 'tls'],
      capacity: { maxConcurrent: 8, maxBrowserConcurrent: 0, available: 8, availableBrowser: 0 },
      labels: { tier: 'office', owner: 'it-ops' },
    }),
    agent('agent-sha-02', 'loc-shanghai', 'sha-office-2', 'probe-2.sha.acme.corp', {
      status: 'offline',
      capabilities: ['http', 'tcp', 'dns'],
      capacity: { maxConcurrent: 8, maxBrowserConcurrent: 0, available: 0, availableBrowser: 0 },
      lastHeartbeatAt: now - 5 * HOUR,
      labels: { tier: 'office', owner: 'it-ops' },
    }),
  ];
}

interface MonitorSeed {
  id: string;
  name: string;
  description: string;
  kind: MonitorKind;
  state: MonitorState;
  lifecycle?: SyntheticMonitor['lifecycle'];
  tags: string[];
  every: number;
  locationIds: string[];
  target: string;
  /** Typical response time in milliseconds, and the share of runs that fail. */
  latency: number;
  failureRate: number;
}

const MONITOR_SEEDS: MonitorSeed[] = [
  { id: 'mon-checkout-api', name: 'Checkout API', description: 'Places a test order end to end.', kind: 'http', state: 'healthy', tags: ['checkout', 'tier-1'], every: 60, locationIds: ['loc-us-east', 'loc-eu-central', 'loc-ap-southeast'], target: 'https://api.acme.example/v1/checkout/health', latency: 184, failureRate: 0.004 },
  { id: 'mon-login-journey', name: 'Sign-in journey', description: 'Opens the app, signs in and waits for the dashboard.', kind: 'browser', state: 'failing', tags: ['web', 'tier-1'], every: 300, locationIds: ['loc-us-east', 'loc-eu-central'], target: 'https://app.acme.example/signin', latency: 4_820, failureRate: 0.42 },
  { id: 'mon-payments-gateway', name: 'Payments gateway', description: 'Authorizes a zero-amount card against the provider sandbox.', kind: 'http', state: 'degraded', tags: ['payments', 'tier-1'], every: 60, locationIds: ['loc-us-east', 'loc-eu-central', 'loc-ap-southeast'], target: 'https://pay.acme.example/v2/authorize', latency: 912, failureRate: 0.06 },
  { id: 'mon-search-api', name: 'Search API', description: 'Runs a known query and checks the first result.', kind: 'http', state: 'healthy', tags: ['search'], every: 120, locationIds: ['loc-us-east', 'loc-eu-central'], target: 'https://api.acme.example/v1/search?q=probe', latency: 96, failureRate: 0.002 },
  { id: 'mon-postgres-primary', name: 'Postgres primary', description: 'TCP reachability of the primary from the office network.', kind: 'tcp', state: 'healthy', tags: ['database'], every: 60, locationIds: ['loc-shanghai'], target: 'db-primary.acme.corp:5432', latency: 12, failureRate: 0 },
  { id: 'mon-edge-dns', name: 'api.acme.example DNS', description: 'A and AAAA records resolve with DNSSEC.', kind: 'dns', state: 'healthy', tags: ['edge'], every: 300, locationIds: ['loc-us-east', 'loc-eu-central', 'loc-ap-southeast'], target: 'api.acme.example', latency: 28, failureRate: 0.001 },
  { id: 'mon-edge-tls', name: 'Edge certificate', description: 'The public certificate has at least 21 days left.', kind: 'tls', state: 'flaky', tags: ['edge'], every: 3_600, locationIds: ['loc-us-east', 'loc-ap-southeast'], target: 'app.acme.example:443', latency: 240, failureRate: 0.12 },
  { id: 'mon-orders-grpc', name: 'Orders gRPC health', description: 'grpc.health.v1 on the orders service.', kind: 'grpc', state: 'healthy', tags: ['orders'], every: 60, locationIds: ['loc-us-east'], target: 'orders.acme.example:443', latency: 41, failureRate: 0.003 },
  { id: 'mon-bastion-ssh', name: 'Bastion SSH', description: 'Banner and host key of the bastion host.', kind: 'ssh', state: 'healthy', tags: ['infra'], every: 600, locationIds: ['loc-shanghai'], target: 'bastion.acme.corp:22', latency: 63, failureRate: 0 },
  { id: 'mon-nightly-export', name: 'Nightly export', description: 'The export job reports in once a night.', kind: 'heartbeat', state: 'unknown', tags: ['batch'], every: 86_400, locationIds: [], target: 'heartbeat', latency: 0, failureRate: 0 },
  { id: 'mon-legacy-ping', name: 'Legacy VPN gateway', description: 'Paused while the gateway is being replaced.', kind: 'icmp', state: 'unknown', lifecycle: 'paused', tags: ['infra'], every: 60, locationIds: ['loc-shanghai'], target: 'vpn-old.acme.corp', latency: 18, failureRate: 0 },
];

function monitorSpec(seed: MonitorSeed): MonitorSpec {
  const literal = (value: string) => ({ source: 'literal' as const, value });
  const [host = seed.target, port = '443'] = seed.target.split(':');

  switch (seed.kind) {
    case 'http':
      return {
        kind: 'http',
        configuration: {
          steps: [
            {
              id: 'step-1',
              name: seed.name,
              method: 'GET',
              url: literal(seed.target),
              headers: [{ name: 'accept', value: literal('application/json') }],
              query: [],
              extractions: [],
              assertions: [
                { id: 'a-status', name: 'Status is 200', source: 'status', severity: 'critical', operator: { operator: 'equals', expected: '200' } },
                { id: 'a-latency', name: 'Under 800 ms', source: 'duration_ms', severity: 'warning', operator: { operator: 'less_than', expected: 800 } },
              ],
            },
          ],
          followRedirects: true,
          maxRedirects: 5,
          verifyTls: true,
        },
      };
    case 'browser':
      return {
        kind: 'browser',
        configuration: {
          steps: [
            { id: 's1', name: 'Open the sign-in page', action: { action: 'goto', url: seed.target } },
            { id: 's2', name: 'Fill in the credentials', action: { action: 'fill', selector: '#email' } },
            { id: 's3', name: 'Wait for the dashboard', action: { action: 'wait_for', selector: '[data-page="home"]' } },
          ],
          viewport: { width: 1440, height: 900 },
          captureScreenshotOnFailure: true,
          captureHarOnFailure: true,
          captureTraceOnFailure: false,
        },
      };
    case 'tcp':
      return { kind: 'tcp', configuration: { host: literal(host), port: Number(port), useTls: false, assertions: [] } };
    case 'ssh':
      return { kind: 'ssh', configuration: { host: literal(host), port: Number(port), expectedIdentificationRegex: '^SSH-2\\.0-OpenSSH' } };
    case 'dns':
      return { kind: 'dns', configuration: { name: seed.target, recordType: 'A', requireDnssec: true, expectedValues: [], assertions: [] } };
    case 'icmp':
      return { kind: 'icmp', configuration: { host: seed.target, count: 4, intervalMillis: 250, maxPacketLossRatio: 0.25 } };
    case 'tls':
      return { kind: 'tls', configuration: { host, port: Number(port), minimumDaysRemaining: 21, expectedSans: [host] } };
    case 'grpc':
      return { kind: 'grpc', configuration: { endpoint: seed.target, useTls: true, metadata: [], call: { kind: 'health', service: 'orders.v1.Orders' }, assertions: [] } };
    case 'heartbeat':
      return { kind: 'heartbeat' };
  }
}

function monitors(now: number): SyntheticMonitor[] {
  return MONITOR_SEEDS.map((seed, index) => ({
    id: seed.id,
    organizationId: ORG,
    name: seed.name,
    description: seed.description,
    kind: seed.kind,
    lifecycle: seed.lifecycle ?? 'active',
    state: seed.state,
    tags: seed.tags,
    activeRevisionId: `${seed.id}-rev-3`,
    ...(seed.lifecycle === 'paused' ? {} : { nextDueAt: now + ((index * 7) % 50 + 4) * 1_000_000 }),
    createdBy: 'u1',
    createdAt: now - (90 - index * 4) * DAY,
    updatedAt: now - (index + 1) * 3 * HOUR,
  }));
}

function revisions(seed: MonitorSeed, now: number): MonitorRevision[] {
  return [3, 2, 1].map((number) => ({
    id: `${seed.id}-rev-${number}`,
    organizationId: ORG,
    monitorId: seed.id,
    number,
    spec: monitorSpec(seed),
    schedule:
      seed.kind === 'heartbeat'
        ? { kind: 'heartbeat', expectedSeconds: seed.every, graceSeconds: 1_800 }
        : { kind: 'interval', everySeconds: seed.every },
    timeoutMillis: seed.kind === 'browser' ? 45_000 : 10_000,
    maxRetries: 1,
    consecutiveFailures: 2,
    consecutiveRecoveries: 2,
    freshnessSeconds: seed.every * 3,
    locationPolicy: seed.locationIds.length > 2 ? { kind: 'majority' } : { kind: 'any' },
    locationIds: seed.locationIds,
    alertOnDegraded: true,
    alertOnFlaky: false,
    alertOnNoData: true,
    injectTraceContext: true,
    tracerouteOnFailure: true,
    lastTestPassedAt: now - (4 - number) * 9 * DAY,
    createdBy: 'u1',
    createdAt: now - (4 - number) * 9 * DAY,
    contentHash: `sha256:${seed.id}:${number}`,
  }));
}

function outcomeFor(seed: MonitorSeed, roll: number, recent: boolean): ProbeOutcome {
  // What the monitor's state says of it shows in its latest runs, not only in the odds.
  if (recent && seed.state === 'failing') return 'failing';
  if (recent && seed.state === 'degraded') return roll < 0.5 ? 'degraded' : 'healthy';
  if (roll < seed.failureRate) return seed.state === 'flaky' ? 'flaky' : 'failing';
  if (roll < seed.failureRate * 2.5) return 'degraded';

  return 'healthy';
}

type PhaseKey = Exclude<keyof TimingBreakdown, 'totalTime'>;

/**
 * The share of an attempt each kind of check spends in the phases its probe
 * times. A phase is a duration of its own, so the shares of a kind stay under
 * the whole attempt; the rest is the response body and local work.
 *
 * A kind lists only what its probe reports. A gRPC call hides everything after
 * the lookup inside its client, and a browser run times the journey as a whole.
 */
const PHASE_SHARES: Partial<Record<MonitorKind, Partial<Record<PhaseKey, number>>>> = {
  http: { dnsTime: 0.06, connectTime: 0.09, tlsTime: 0.21, firstByteTime: 0.52 },
  tcp: { dnsTime: 0.24, connectTime: 0.68 },
  dns: { dnsTime: 0.94 },
  tls: { dnsTime: 0.07, connectTime: 0.21, tlsTime: 0.64 },
  grpc: { dnsTime: 0.09 },
  ssh: { dnsTime: 0.08, connectTime: 0.27, firstByteTime: 0.56 },
};

function timing(kind: MonitorKind, total: number): TimingBreakdown {
  const phases = Object.entries(PHASE_SHARES[kind] ?? {}).map(([phase, share]) => [
    phase,
    Math.round(total * share),
  ]);

  return { ...Object.fromEntries(phases), totalTime: total };
}

/** The route a failed HTTP run traced, breaking off after the provider's edge. */
const TRACEROUTE_HOPS = JSON.stringify([
  [1, '10.20.0.1', 1, 4, 4, 0.4, 0.6, 1.1],
  [2, '100.64.12.1', 1, 4, 4, 2.1, 2.8, 3.9],
  [3, '203.0.113.1', 2, 4, 4, 8.7, 9.4, 11.2],
  [4, null, 0, 4, 0, null, null, null],
  [5, '198.51.100.17', 1, 4, 1, 31.5, 31.5, 31.5],
]);

/** What an HTTP run's attempt left as steps: its request, and the traceroute a failure ran. */
function httpEvidence(
  run: number,
  startedAt: number,
  total: number,
  failed: boolean,
): NonNullable<ProbeAttempt['evidence']> {
  const request = {
    stepId: 'step-1',
    name: 'Request',
    action: 'http',
    startedAt: startedAt,
    finishedAt: startedAt + total,
    outcome: failed ? ('failing' as const) : ('healthy' as const),
    ...(failed ? { errorCategory: 'timeout', errorMessage: 'timed out after 10000 ms' } : {}),
    metadata: {
      ...(failed ? {} : { status: '200', httpVersion: 'HTTP/2' }),
      remote_address: '203.0.113.10:443',
      trace_id: `4bf92f3577b34da6a3ce929d0e0e${run.toString(16).padStart(4, '0')}`,
    },
  };

  if (!failed) return [request];

  return [
    request,
    {
      stepId: 'traceroute',
      name: 'Traceroute to 203.0.113.10',
      action: 'traceroute',
      startedAt: startedAt + total,
      finishedAt: startedAt + total + 4_100_000,
      outcome: 'unknown',
      metadata: {
        target: '203.0.113.10',
        hops: TRACEROUTE_HOPS,
        reached: 'false',
        mode: 'unprivileged',
        rounds: '4',
      },
    },
  ];
}

function results(now: number): SyntheticResult[] {
  const items: SyntheticResult[] = [];
  const random = sequence(20_260_523);

  for (const seed of MONITOR_SEEDS) {
    if (seed.kind === 'heartbeat' || seed.lifecycle === 'paused') continue;

    // The last runs of every monitor at each of its locations, newest first.
    const runs = Math.min(36, Math.max(6, Math.round(6 * HOUR / 1_000_000 / seed.every)));

    for (let run = 0; run < runs; run += 1) {
      for (const locationId of seed.locationIds) {
        const startedAt = now - run * seed.every * 1_000_000 - Math.round(random() * 4_000_000);
        const outcome = outcomeFor(seed, random(), run < 3);
        const slow = outcome === 'degraded' ? 2.6 : outcome === 'failing' ? 4 : 1;
        const regional = locationId === 'loc-ap-southeast' ? 1.7 : locationId === 'loc-eu-central' ? 1.15 : 1;
        const total = Math.round(seed.latency * slow * regional * (0.82 + random() * 0.4) * 1_000);
        const failed = outcome === 'failing' || outcome === 'flaky';

        items.push({
          id: `res-${seed.id}-${locationId}-${run}`,
          organizationId: ORG,
          monitorId: seed.id,
          monitorRevisionId: `${seed.id}-rev-3`,
          locationId: locationId,
          agentId: agents(now).find((agent) => agent.locationId === locationId)?.id,
          taskId: `task-${seed.id}-${run}`,
          isTest: false,
          resultSequence: 48_210 - run,
          scheduledAt: startedAt - 200_000,
          startedAt: startedAt,
          finishedAt: startedAt + total,
          receivedAt: startedAt + total + 150_000,
          outcome,
          attempts: [
            {
              number: 1,
              startedAt: startedAt,
              finishedAt: startedAt + total,
              outcome,
              timing: timing(seed.kind, total),
              ...(failed
                ? {
                    errorCategory: seed.kind === 'browser' ? 'assertion_failed' : 'timeout',
                    errorMessage:
                      seed.kind === 'browser'
                        ? 'Timed out after 30000 ms waiting for [data-page="home"]'
                        : 'Deadline exceeded while reading the response headers',
                  }
                : {}),
              metadata: seed.kind === 'http' ? { status: failed ? '504' : '200', protocol: 'h2' } : {},
              ...(seed.kind === 'http' ? { evidence: httpEvidence(run, startedAt, total, failed) } : {}),
            },
          ],
          assertions:
            seed.kind === 'http'
              ? [
                  { assertionId: 'a-status', severity: 'critical', passed: !failed, actual: failed ? '504' : '200' },
                  { assertionId: 'a-latency', severity: 'warning', passed: total < 800_000, actual: String(Math.round(total / 1_000)) },
                ]
              : [],
          secretVersions: {},
          protocolVersion: 3,
          metadata: {},
        } as SyntheticResult);
      }
    }
  }

  return items.sort((left, right) => right.startedAt - left.startedAt);
}

function agentTokens(now: number): ProbeAgentToken[] {
  return [
    {
      id: 'tok-sha-pool',
      organizationId: ORG,
      locationId: 'loc-shanghai',
      name: 'Shanghai office pool',
      tokenPrefix: 'mspat-7qx2k7m2q9',
      status: 'active',
      expiresAt: now + 200 * DAY,
      lastUsedAt: now - 40 * DAY,
      createdBy: 'u1',
      createdAt: now - 41 * DAY,
      updatedAt: now - 40 * DAY,
    },
  ];
}

function secrets(now: number): SyntheticSecret[] {
  const secret = (id: string, name: string, description: string, version: number, age: number): SyntheticSecret => ({
    id,
    organizationId: ORG,
    name,
    description,
    currentVersion: version,
    createdBy: 'u1',
    createdAt: now - (age + 60) * DAY,
    updatedAt: now - age * DAY,
  });

  return [
    secret('sec-probe-password', 'PROBE_USER_PASSWORD', 'Password of the synthetic sign-in user.', 4, 12),
    secret('sec-payments-key', 'PAYMENTS_SANDBOX_KEY', 'API key of the payment provider sandbox.', 2, 48),
    secret('sec-bastion-key', 'BASTION_PRIVATE_KEY', 'Read-only key for the bastion banner check.', 1, 96),
  ];
}

// ───────────────────────────── Status pages ─────────────────────────────

function statusPages(now: number): StatusPage[] {
  const page = (id: string, name: string, slug: string, overrides: Partial<StatusPage> = {}): StatusPage => ({
    id,
    orgId: ORG,
    name,
    slug,
    logoUrl: null,
    brandColor: '#3b46c4',
    customDomain: null,
    timezone: 'UTC',
    language: 'en-us',
    languages: ['en-us', 'zh-cn'],
    historyDays: 90,
    deliveryRetentionDays: 30,
    privateSessionDays: 7,
    visibility: 'public',
    lifecycle: 'active',
    archivedAt: null,
    purgeAfter: null,
    createdAt: now - 200 * DAY,
    updatedAt: now - 38 * MINUTE,
    ...overrides,
  });

  return [
    page('sp-acme-cloud', 'Acme Cloud', 'acme-cloud', { customDomain: 'status.acme.example' }),
    page('sp-internal', 'Internal platform', 'acme-internal', {
      visibility: 'private',
      brandColor: '#0f766e',
      languages: ['en-us'],
      createdAt: now - 60 * DAY,
      updatedAt: now - 6 * DAY,
    }),
    page('sp-legacy', 'Acme Classic', 'acme-classic', {
      lifecycle: 'archived',
      archivedAt: now - 20 * DAY,
      purgeAfter: now + 10 * DAY,
      updatedAt: now - 20 * DAY,
    }),
  ];
}

const COMPONENT_SEEDS: Array<[string, string, string, ComponentStatus]> = [
  ['cmp-api', 'Public API', 'REST and gRPC endpoints at api.acme.example.', 'partial_outage'],
  ['cmp-dashboard', 'Web dashboard', 'The application at app.acme.example.', 'degraded_performance'],
  ['cmp-ingest', 'Data ingestion', 'OTLP, Prometheus remote write and log shippers.', 'operational'],
  ['cmp-alerting', 'Alerting and notifications', 'Rule evaluation and delivery to on-call.', 'operational'],
  ['cmp-payments', 'Billing and payments', 'Invoices, plan changes and the payment provider.', 'operational'],
  ['cmp-eu', 'EU region', 'Everything served from Frankfurt.', 'maintenance'],
];

/** What the components of the first page depend on, and how they are doing. */
const COMPONENT_DEPENDENCIES: Record<string, Pick<StatusPageComponent, 'dependencies' | 'dependencyMode' | 'dependencyHealth'>> = {
  'cmp-api': {
    dependencies: [
      { kind: 'technical_service', target: 'api-gateway', impact: 'outage' },
      { kind: 'alert_rule', target: 'rule-api-5xx', impact: 'outage' },
    ],
    dependencyMode: 'follow',
    dependencyHealth: {
      status: 'partial_outage',
      causes: [{ kind: 'alert_rule', target: 'rule-api-5xx', status: 'partial_outage', incidentCount: 1 }],
      changedAt: Date.parse('2026-05-23T09:20:00Z') * 1000,
      evaluatedAt: Date.parse('2026-05-23T09:58:00Z') * 1000,
    },
  },
  'cmp-payments': {
    dependencies: [{ kind: 'technical_service', target: 'payments-gateway', impact: 'outage' }],
    dependencyMode: 'automation',
    dependencyHealth: null,
  },
};

function components(pageId: string, now: number): StatusPageComponent[] {
  return COMPONENT_SEEDS.map(([id, name, description, status], position) => ({
    id,
    orgId: ORG,
    statusPageId: pageId,
    name,
    description,
    status,
    visibility: 'enabled',
    lifecycle: 'active',
    position,
    archivedAt: null,
    createdAt: now - 200 * DAY,
    updatedAt: now - 38 * MINUTE,
    ...(pageId === 'sp-acme-cloud' ? COMPONENT_DEPENDENCIES[id] : undefined),
  }));
}

/**
 * Each component's state since the page was made, as the backend records it: one event per
 * change, every one ending where the next begins. Mostly operational, with the incidents in it.
 */
function componentStatusEvents(pageId: string, now: number): StatusPageComponentStatusEvent[] {
  // What interrupted each component, oldest first: [status, started ago, ended ago or still on].
  const interruptions: Record<string, Array<[ComponentStatus, number, number | null]>> = {
    'cmp-api': [
      ['major_outage', 19 * DAY + 5 * HOUR, 19 * DAY + 4 * HOUR],
      ['degraded_performance', 19 * DAY + 4 * HOUR, 19 * DAY + 2 * HOUR],
      ['partial_outage', 38 * MINUTE, null],
    ],
    'cmp-dashboard': [
      ['partial_outage', 71 * DAY + 1 * HOUR, 71 * DAY],
      ['degraded_performance', 31 * MINUTE, null],
    ],
    'cmp-ingest': [['degraded_performance', 6 * DAY + 3 * HOUR, 6 * DAY + 1 * HOUR]],
    'cmp-alerting': [['partial_outage', 33 * DAY + 2 * HOUR, 33 * DAY + 1 * HOUR]],
    'cmp-payments': [['degraded_performance', 47 * DAY + 8 * HOUR, 47 * DAY + 5 * HOUR]],
    'cmp-eu': [
      ['maintenance', 58 * DAY + 2 * HOUR, 58 * DAY],
      ['maintenance', 20 * MINUTE, null],
    ],
  };
  const events: StatusPageComponentStatusEvent[] = [];
  const event = (componentId: string, status: ComponentStatus, startedAgo: number, endedAgo: number | null) =>
    events.push({
      id: `evt-${componentId}-${events.length}`,
      orgId: ORG,
      statusPageId: pageId,
      componentId: componentId,
      status,
      startedAt: now - startedAgo,
      endedAt: endedAgo === null ? null : now - endedAgo,
      createdAt: now - startedAgo,
    });

  for (const [componentId, list] of Object.entries(interruptions)) {
    let since = 200 * DAY;

    for (const [status, startedAgo, endedAgo] of list) {
      // Working normally up to the interruption, unless one ran straight into the next.
      if (since > startedAgo) event(componentId, 'operational', since, startedAgo);
      event(componentId, status, startedAgo, endedAgo);
      since = endedAgo ?? 0;
    }

    if (since > 0) event(componentId, 'operational', since, null);
  }

  return events;
}

function incidents(pageId: string, now: number): StatusPageIncident[] {
  const incident = (
    id: string,
    title: string,
    overrides: Partial<StatusPageIncident>,
    updates: Array<[StatusPageIncident['status'], string, number]>,
  ): StatusPageIncident => ({
    id,
    orgId: ORG,
    statusPageId: pageId,
    sourceIncidentId: null,
    kind: 'incident',
    title,
    impact: 'major',
    status: updates[0]![0],
    publicationState: 'published',
    draftMessage: null,
    componentIds: ['cmp-api'],
    updates: updates.map(([status, message, ago], index) => ({
      id: `${id}-u${updates.length - index}`,
      orgId: ORG,
      statusPageId: pageId,
      incidentId: id,
      status,
      message,
      createdAt: now - ago,
    })),
    startedAt: now - updates.at(-1)![2],
    endedAt: null,
    publishedAt: now - updates.at(-1)![2],
    createdAt: now - updates.at(-1)![2],
    updatedAt: now - updates[0]![2],
    ...overrides,
  });

  return [
    incident('inc-api-errors', 'Elevated error rates on the public API', { impact: 'major', componentIds: ['cmp-api', 'cmp-dashboard'] }, [
      ['identified', 'A configuration change to the edge proxies is rejecting a share of requests with 502. We are rolling it back region by region.', 12 * MINUTE],
      ['investigating', 'Roughly one request in six to the public API fails. The dashboard is slower than usual because it retries them.', 27 * MINUTE],
      ['investigating', 'We are looking into reports of failed API requests.', 38 * MINUTE],
    ]),
    incident('mnt-eu-database', 'Database upgrade in the EU region', { kind: 'maintenance', impact: 'maintenance', componentIds: ['cmp-eu'], startedAt: now - 20 * MINUTE }, [
      ['in_progress', 'The upgrade has started. Writes in the EU region may take a few seconds longer during the switch-over.', 20 * MINUTE],
      ['scheduled', 'We will upgrade the primary database of the EU region. No downtime is expected.', 3 * DAY],
    ]),
    incident('mnt-billing-export', 'Billing export maintenance', { kind: 'maintenance', impact: 'maintenance', componentIds: ['cmp-payments'], startedAt: now + 2 * DAY }, [
      ['scheduled', 'Invoice exports will be unavailable for about 30 minutes while the export service is moved.', 1 * DAY],
    ]),
    incident('inc-draft-search', 'Search results delayed', { impact: 'minor', publicationState: 'draft', publishedAt: null, draftMessage: 'Indexing is running about four minutes behind.', componentIds: ['cmp-dashboard'] }, [
      ['investigating', 'Indexing is running about four minutes behind.', 9 * MINUTE],
    ]),
    incident('inc-ingest-lag', 'Delayed ingestion of logs', { impact: 'minor', componentIds: ['cmp-ingest'], endedAt: now - (6 * DAY + 1 * HOUR) }, [
      ['resolved', 'The backlog has been processed and ingestion is back to real time.', 6 * DAY + 1 * HOUR],
      ['monitoring', 'Capacity has been added and the backlog is draining.', 6 * DAY + 2 * HOUR],
      ['investigating', 'Logs are arriving several minutes late.', 6 * DAY + 3 * HOUR],
    ]),
    incident('inc-api-outage', 'Public API unavailable', { impact: 'critical', componentIds: ['cmp-api'], endedAt: now - (19 * DAY + 2 * HOUR) }, [
      ['resolved', 'All requests have been served normally for an hour. A full write-up will follow.', 19 * DAY + 2 * HOUR],
      ['monitoring', 'Traffic has been moved to the standby load balancers and requests are succeeding again.', 19 * DAY + 4 * HOUR],
      ['identified', 'A failed certificate rotation took the primary load balancers out of service.', 19 * DAY + 4.5 * HOUR],
      ['investigating', 'The public API is not answering.', 19 * DAY + 5 * HOUR],
    ]),
    incident('inc-notify-delay', 'Notifications delivered late', { impact: 'major', componentIds: ['cmp-alerting'], endedAt: now - (33 * DAY + 1 * HOUR) }, [
      ['resolved', 'The queue is empty and notifications are delivered within seconds again.', 33 * DAY + 1 * HOUR],
      ['investigating', 'Some alert notifications are delivered up to ten minutes late.', 33 * DAY + 2 * HOUR],
    ]),
    incident('mnt-eu-network', 'Network maintenance in the EU region', { kind: 'maintenance', impact: 'maintenance', componentIds: ['cmp-eu'], endedAt: now - 58 * DAY }, [
      ['completed', 'The maintenance is complete.', 58 * DAY],
      ['in_progress', 'Maintenance is under way.', 58 * DAY + 2 * HOUR],
    ]),
  ];
}

const isOpen = (incident: StatusPageIncident) => incident.endedAt === null;

const isAnnounced = (incident: StatusPageIncident, now: number) =>
  incident.kind === 'maintenance' && (incident.status === 'scheduled' || incident.startedAt > now);

function snapshot(page: StatusPage, now: number): StatusPageSnapshot {
  const all = incidents(page.id, now).filter((incident) => incident.publicationState === 'published');
  // The second page is quiet: everything works and nothing is planned.
  const quiet = page.id !== 'sp-acme-cloud';

  return {
    page,
    overallStatus: quiet ? 'operational' : 'partial_outage',
    components: components(page.id, now).map((component) => (quiet ? { ...component, status: 'operational' } : component)),
    componentStatusEvents: quiet ? [] : componentStatusEvents(page.id, now),
    // As the server sorts them: maintenance that has not begun is announced, everything else
    // that is still open is going on now, maintenance under way included.
    activeIncidents: quiet ? [] : all.filter((incident) => isOpen(incident) && !isAnnounced(incident, now)),
    scheduledMaintenance: quiet ? [] : all.filter((incident) => isOpen(incident) && isAnnounced(incident, now)),
    history: quiet ? [] : all.filter((incident) => !isOpen(incident)),
    updatedAt: now - 12 * MINUTE,
    generatedAt: now,
  };
}

function publicSnapshot(page: StatusPage, now: number): PublicStatusPageSnapshot {
  const full = snapshot(page, now);
  const publicIncident = (incident: StatusPageIncident) => ({
    id: incident.id,
    kind: incident.kind,
    title: incident.title,
    impact: incident.impact,
    status: incident.status,
    componentIds: incident.componentIds,
    startedAt: incident.startedAt,
    endedAt: incident.endedAt,
    updates: incident.updates.map(({ id, status, message, createdAt: createdAt }) => ({ id, status, message, createdAt: createdAt })),
  });

  return {
    page: {
      name: page.name,
      slug: page.slug,
      logoUrl: page.logoUrl,
      brandColor: page.brandColor,
      timezone: page.timezone,
      language: page.language,
      languages: page.languages,
      historyDays: page.historyDays,
      visibility: page.visibility,
    },
    overallStatus: full.overallStatus,
    components: full.components.map(({ id, name, description, status }) => ({ id, name, description, status })),
    componentStatusEvents: full.componentStatusEvents.map(({ componentId: componentId, status, startedAt: startedAt, endedAt: endedAt }) => ({ componentId: componentId, status, startedAt: startedAt, endedAt: endedAt })),
    activeIncidents: full.activeIncidents.map(publicIncident),
    scheduledMaintenance: full.scheduledMaintenance.map(publicIncident),
    history: full.history.map(publicIncident),
    updatedAt: full.updatedAt,
    generatedAt: now,
  };
}

function subscribers(now: number): StatusPageSubscriber[] {
  const targets: Array<[StatusPageSubscriber['channel'], string, StatusPageSubscriber['status'], number]> = [
    ['email', 'a***@northwind.example', 'active', 120],
    ['email', 'o***@contoso.example', 'active', 96],
    ['webhook', 'https://hooks.fabrikam.example/***', 'active', 88],
    ['email', 'p***@tailspin.example', 'active', 61],
    ['email', 's***@wingtip.example', 'pending', 1],
    ['email', 'd***@adatum.example', 'active', 44],
    ['webhook', 'https://ops.litware.example/***', 'active', 30],
    ['email', 'm***@proseware.example', 'unsubscribed', 150],
  ];

  return targets.map(([channel, maskedTarget, status, age], index) => ({
    id: `sub-${index + 1}`,
    channel,
    maskedTarget: maskedTarget,
    status,
    confirmationSentAt: now - age * DAY,
    confirmedAt: status === 'pending' ? null : now - age * DAY + 4 * MINUTE,
    unsubscribedAt: status === 'unsubscribed' ? now - 12 * DAY : null,
    createdAt: now - age * DAY,
    updatedAt: now - Math.min(age, 12) * DAY,
  }));
}

function deliveries(now: number): StatusPageDelivery[] {
  const active = subscribers(now).filter((subscriber) => subscriber.status === 'active');

  return active.map((subscriber, index) => ({
    id: `dlv-${index + 1}`,
    subscriberId: subscriber.id,
    channel: subscriber.channel,
    maskedTarget: subscriber.maskedTarget,
    eventKey: 'inc-api-errors:u3',
    status: index === 2 ? 'failed' : index === 5 ? 'pending' : 'delivered',
    attempts: index === 2 ? 4 : 1,
    nextAttemptAt: now + 5 * MINUTE,
    lastError: index === 2 ? 'The endpoint answered 503 Service Unavailable' : null,
    deliveredAt: index === 2 || index === 5 ? null : now - 11 * MINUTE,
    createdAt: now - 12 * MINUTE,
    updatedAt: now - 11 * MINUTE,
  }));
}

function automationRules(pageId: string, now: number): ActiveAutomationRule[] {
  const rule = (id: string, name: string, position: number, sourceKind: 'alert_incident' | 'synthetic_monitor', automatic: boolean): ActiveAutomationRule => ({
    rule: {
      id,
      organizationId: ORG,
      statusPageId: pageId,
      name,
      position,
      lifecycle: 'active',
      activeRevisionId: `${id}-rev-2`,
      draftRevisionId: null,
      createdBy: 'u1',
      createdAt: now - 50 * DAY,
      updatedAt: now - 8 * DAY,
    },
    revision: {
      id: `${id}-rev-2`,
      organizationId: ORG,
      statusPageId: pageId,
      ruleId: id,
      number: 2,
      matchers: {
        sourceKind: sourceKind,
        sourceId: sourceKind === 'synthetic_monitor' ? 'mon-checkout-api' : null,
        labels: sourceKind === 'alert_incident' ? { service: 'api-gateway' } : {},
        minimumSeverity: 'error',
      },
      action: {
        componentIds: ['cmp-api'],
        publicationMode: automatic ? 'automatic' : 'approval',
        sustainedDelaySeconds: 300,
        impactMap: { degraded: 'minor', failing: 'major' },
        templates: {
          title: 'Problems with {{component}}',
          investigating: 'We are investigating problems with {{component}}.',
          update: 'We are still working on {{component}}.',
          resolved: '{{component}} is working normally again.',
        },
        correlationKeyTemplate: '{{source_id}}',
      },
      contentHash: `sha256:${id}`,
      createdBy: 'u1',
      createdAt: now - 8 * DAY,
    },
  });

  return [
    rule('aut-api-alerts', 'API gateway alerts', 0, 'alert_incident', false),
    rule('aut-checkout-check', 'Checkout API check', 1, 'synthetic_monitor', true),
  ];
}

function automationCandidates(pageId: string, now: number): AutomationCandidate[] {
  return [
    {
      id: 'cand-api-gateway',
      organizationId: ORG,
      statusPageId: pageId,
      ruleRevisionId: 'aut-api-alerts-rev-2',
      correlationKey: 'api-gateway',
      state: 'pending_approval',
      title: 'Problems with Public API',
      message: 'We are investigating problems with Public API.',
      resolvedMessage: 'Public API is working normally again.',
      impact: 'major',
      componentIds: ['cmp-api'],
      automatic: false,
      statusIncidentId: null,
      dueAt: now - 4 * MINUTE,
      createdAt: now - 9 * MINUTE,
      updatedAt: now - 4 * MINUTE,
      lastError: null,
    },
  ];
}

// ───────────────────────────── Routes ─────────────────────────────

/** How a set of mock runs went, as the availability endpoint sums them. */
function summarize(runs: SyntheticResult[]) {
  const available = runs.filter((run) => ['healthy', 'flaky', 'degraded'].includes(run.outcome)).length;
  const failing = runs.filter((run) => run.outcome === 'failing').length;
  const durations = runs
    .map((run) => run.attempts.at(-1)?.timing.totalTime)
    .filter((micros): micros is number => micros !== undefined)
    .map((micros) => micros / 1_000)
    .sort((left, right) => left - right);
  const at = (share: number) =>
    durations.length > 0 ? durations[Math.min(durations.length - 1, Math.floor(share * durations.length))]! : null;

  return {
    runs: runs.length,
    available,
    failing,
    undecided: runs.length - available - failing,
    availability: available + failing > 0 ? available / (available + failing) : null,
    p50Ms: at(0.5),
    p95Ms: at(0.95),
    maxMs: durations.at(-1) ?? null,
  };
}

/** The availability endpoint over the mock runs: in all, by check, by location and step by step. */
function availability(all: SyntheticResult[], from: number, to: number, monitorId: string) {
  const runs = all.filter(
    (run) => !run.isTest && run.startedAt >= from && run.startedAt < to && (!monitorId || run.monitorId === monitorId),
  );
  const step = Math.max(60_000_000, Math.ceil((to - from) / 48 / 60_000_000) * 60_000_000);
  const groupBy = (key: (run: SyntheticResult) => string) => {
    const groups = new Map<string, SyntheticResult[]>();
    for (const run of runs) groups.set(key(run), [...(groups.get(key(run)) ?? []), run]);
    return [...groups.entries()];
  };
  const stepOf = (run: SyntheticResult) => String(from + Math.floor((run.startedAt - from) / step) * step);

  return {
    from,
    to,
    step,
    total: summarize(runs),
    checks: groupBy((run) => run.monitorId).map(([checkId, group]) => ({ checkId: checkId, ...summarize(group) })),
    locations: groupBy((run) => run.locationId).map(([locationId, group]) => ({ locationId: locationId, ...summarize(group) })),
    buckets: groupBy(stepOf)
      .map(([start, group]) => ({ start: Number(start), ...summarize(group) }))
      .sort((left, right) => left.start - right.start),
    locationBuckets: monitorId
      ? groupBy((run) => `${run.locationId} ${stepOf(run)}`)
          .map(([key, group]) => {
            const [location_id, start] = key.split(' ');
            return { locationId: location_id!, start: Number(start), ...summarize(group) };
          })
          .sort((left, right) => left.start - right.start)
      : [],
  };
}

const paged = <T>(req: Request, items: T[]) => {
  const page = Math.max(1, Number(req.query.page ?? 1));
  const perPage = Math.max(1, Number(req.query.perPage ?? 25));

  return { items: items.slice((page - 1) * perPage, page * perPage), page, perPage: perPage, total: items.length };
};

/** Registers the Synthetics and Status pages endpoints on the mock backend. */
export function registerReliabilityRoutes(app: Express, nowIso: string): void {
  const now = Date.parse(nowIso) * 1_000;
  const seedOf = (id: string) => MONITOR_SEEDS.find((seed) => seed.id === id) ?? MONITOR_SEEDS[0]!;
  const monitorOf = (id: string) => monitors(now).find((monitor) => monitor.id === id) ?? monitors(now)[0]!;
  const pageOf = (id: string) => statusPages(now).find((page) => page.id === id || page.slug === id) ?? statusPages(now)[0]!;

  app.get('/api/v1/synthetics/monitors', (_req, res) => res.json(monitors(now)));
  app.get('/api/v1/synthetics/monitors/:id', (req, res) =>
    res.json({ monitor: monitorOf(req.params.id), revisions: revisions(seedOf(req.params.id), now) }),
  );
  app.get('/api/v1/synthetics/monitors/:id/results', (req, res) => {
    const limit = Number(req.query.limit ?? 100);
    res.json(results(now).filter((result) => result.monitorId === seedOf(req.params.id).id).slice(0, limit));
  });
  app.get('/api/v1/synthetics/results', (req, res) => {
    const outcome = typeof req.query.outcome === 'string' ? req.query.outcome : '';
    const monitorId = typeof req.query.monitorId === 'string' ? req.query.monitorId : '';
    const from = Number(req.query.fromTime ?? Number.NEGATIVE_INFINITY);
    const to = Number(req.query.toTime ?? Number.POSITIVE_INFINITY);
    const filtered = results(now).filter(
      (result) =>
        (!outcome || result.outcome === outcome) &&
        (!monitorId || result.monitorId === monitorId) &&
        result.finishedAt >= from &&
        result.finishedAt < to,
    );
    res.json(paged(req, filtered));
  });
  app.get('/api/v1/synthetics/availability', (req, res) => {
    const to = Number(req.query.to ?? now);
    const from = Number(req.query.from ?? to - 24 * 60 * 60 * 1_000_000);
    const monitorId = typeof req.query.monitorId === 'string' ? req.query.monitorId : '';
    res.json(availability(results(now), from, to, monitorId));
  });
  app.get('/api/v1/synthetics/locations', (_req, res) => res.json(locations(now)));
  app.get('/api/v1/synthetics/locations/:id/agents', (req, res) =>
    res.json(agents(now).filter((agent) => agent.locationId === req.params.id)),
  );
  app.get('/api/v1/synthetics/agents', (_req, res) => res.json(agents(now)));
  app.get('/api/v1/synthetics/agent-tokens', (_req, res) => res.json(agentTokens(now)));
  app.get('/api/v1/synthetics/secrets', (_req, res) => res.json(secrets(now)));

  app.get('/api/v1/status-pages', (req, res) => {
    const lifecycle = req.query.lifecycle === 'archived' ? 'archived' : 'active';
    res.json(statusPages(now).filter((page) => page.lifecycle === lifecycle));
  });
  // Across every page, for the bell in the top bar. Registered before the `:id` routes.
  app.get('/api/v1/status-pages/automation/candidates/pending', (_req, res) =>
    res.json(automationCandidates('sp-acme-cloud', now)),
  );
  app.get('/api/v1/status-pages/:id', (req, res) => res.json(snapshot(pageOf(req.params.id), now)));
  app.get('/api/v1/status-pages/:id/incidents', (req, res) => {
    const kind = req.query.kind === 'maintenance' ? 'maintenance' : 'incident';
    const view = typeof req.query.view === 'string' ? req.query.view : 'current';
    const quiet = pageOf(req.params.id).id !== 'sp-acme-cloud';
    const items = (quiet ? [] : incidents(req.params.id, now)).filter((incident) => {
      if (incident.kind !== kind) return false;
      if (view === 'draft') return incident.publicationState === 'draft';
      if (incident.publicationState !== 'published') return false;
      if (view === 'resolved' || view === 'completed') return !isOpen(incident);
      if (view === 'upcoming') return isOpen(incident) && incident.status === 'scheduled';
      if (view === 'in_progress') return isOpen(incident) && incident.status !== 'scheduled';
      return isOpen(incident);
    });
    res.json({ kind, view, items, total: items.length });
  });
  app.get('/api/v1/status-pages/:id/incidents/:incidentId', (req, res) => {
    const all = incidents(req.params.id, now);
    res.json(all.find((incident) => incident.id === req.params.incidentId) ?? all[0]);
  });
  app.get('/api/v1/status-pages/:id/history', (req, res) => {
    const quiet = pageOf(req.params.id).id !== 'sp-acme-cloud';
    res.json(paged(req, quiet ? [] : incidents(req.params.id, now).filter((incident) => !isOpen(incident))));
  });
  app.get('/api/v1/status-pages/:id/subscribers', (req, res) => res.json(paged(req, subscribers(now))));
  app.get('/api/v1/status-pages/:id/deliveries', (req, res) => res.json(paged(req, deliveries(now))));
  app.get('/api/v1/status-pages/:id/domain', (req, res) => {
    const page = pageOf(req.params.id);
    if (!page.customDomain) return res.json(null);

    return res.json({
      config: {
        orgId: ORG,
        statusPageId: page.id,
        hostname: page.customDomain,
        verificationToken: 'ms-v-8f2c',
        state: 'active',
        domainId: 'dom-1',
        routingValid: true,
        lastCheckedAt: now - 20 * MINUTE,
        lastError: null,
        certNotAfter: now + 64 * DAY,
        lastAlertedState: null,
        lastAlertedAt: null,
        createdAt: now - 150 * DAY,
        updatedAt: now - 20 * MINUTE,
      },
      txtName: `_molesignal.${page.customDomain}`,
      txtValue: 'ms-v-8f2c',
      routingTarget: 'status.molesignal.example',
    });
  });
  app.get('/api/v1/status-pages/:id/domain/capability', (_req, res) => res.json({ available: true }));
  app.get('/api/v1/status-pages/:id/access-rules', (req, res) =>
    res.json(
      pageOf(req.params.id).visibility === 'private'
        ? [
            { id: 'rule-1', kind: 'domain', maskedValue: 'acme.example', createdAt: now - 50 * DAY, updatedAt: now - 50 * DAY },
            { id: 'rule-2', kind: 'email', maskedValue: 'a***@partner.example', createdAt: now - 9 * DAY, updatedAt: now - 9 * DAY },
          ]
        : [],
    ),
  );
  app.get('/api/v1/status-pages/:id/access-sessions', (_req, res) => res.json([]));
  app.get('/api/v1/status-pages/:id/slack-app', (_req, res) =>
    res.json({
      connectorId: null,
      redirectUrl: 'https://status.acme.example/api/v1/public/slack/callback',
      connectors: [{ id: 'conn-slack-status', name: 'Acme status Slack app', enabled: true, addsChannels: true }],
    }),
  );
  app.get('/api/v1/status-pages/:id/automation/rules', (req, res) => res.json(automationRules(req.params.id, now)));
  app.get('/api/v1/status-pages/:id/automation/candidates', (req, res) =>
    res.json(pageOf(req.params.id).id === 'sp-acme-cloud' ? automationCandidates(req.params.id, now) : []),
  );
  app.get('/api/v1/status-pages/:id/automation/settings', (_req, res) => res.json({ paused: false }));

  app.get('/api/v1/public/status-pages/:slug', (req, res) => res.json(publicSnapshot(pageOf(req.params.slug), now)));
  app.get('/api/v1/public/status-pages/:slug/access', (req, res) => {
    const page = pageOf(req.params.slug);
    res.json({
      name: page.name,
      slug: page.slug,
      logoUrl: page.logoUrl,
      brandColor: page.brandColor,
      language: page.language,
      languages: page.languages,
      visibility: page.visibility,
    });
  });
}
